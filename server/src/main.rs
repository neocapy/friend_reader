use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, Response, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use epub::doc::EpubDoc;
use sha2::{Digest, Sha256};
use shared::*;
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::time;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

#[derive(Clone)]
struct ServerState {
    document: Arc<Document>,
    images: Arc<HashMap<String, Vec<u8>>>,
    users: Arc<RwLock<HashMap<String, UserData>>>,
    password_hash: Option<String>,
}

struct UserData {
    user: ConnectedUser,
    last_heartbeat: Instant,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: server <epub_file> [--password <password>]");
        std::process::exit(1);
    }

    let epub_path = PathBuf::from(&args[1]);
    let mut password: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--password" => {
                if i + 1 < args.len() {
                    password = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("--password requires a value");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    let password_hash = password.map(|p| {
        let mut hasher = Sha256::new();
        hasher.update(p.as_bytes());
        hex::encode(hasher.finalize())
    });

    if password_hash.is_some() {
        info!("Password protection enabled");
    }

    info!("Loading EPUB from: {:?}", epub_path);
    let (document, images) = parse_epub(&epub_path)?;
    info!("Loaded document with {} elements", document.elements.len());
    info!("Loaded {} images", images.len());

    let state = ServerState {
        document: Arc::new(document),
        images: Arc::new(images),
        users: Arc::new(RwLock::new(HashMap::new())),
        password_hash,
    };

    let heartbeat_state = state.clone();
    tokio::spawn(async move {
        heartbeat_cleanup(heartbeat_state).await;
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/document", get(document_handler))
        .route("/images/{id}", get(image_handler))
        .route("/positions", get(positions_handler))
        .route("/update_position", post(update_position_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 15470));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler(State(state): State<ServerState>) -> impl IntoResponse {
    info!("GET /health");
    Json(HealthResponse {
        status: "ok".to_string(),
        requires_password: state.password_hash.is_some(),
    })
}

async fn document_handler(
    State(state): State<ServerState>,
    Query(auth): Query<AuthRequest>,
) -> Result<Json<Document>, StatusCode> {
    info!("GET /document");
    if !check_auth(&state, auth.password_hash.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json((*state.document).clone()))
}

async fn image_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Query(auth): Query<AuthRequest>,
) -> Result<Response<Body>, StatusCode> {
    info!("GET /images/{}", id);
    if !check_auth(&state, auth.password_hash.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let images = state.images.clone();
    let image_data = images.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let content_type = if id.ends_with(".jpg") || id.ends_with(".jpeg") {
        "image/jpeg"
    } else if id.ends_with(".png") {
        "image/png"
    } else if id.ends_with(".gif") {
        "image/gif"
    } else if id.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(image_data.clone()))
        .unwrap())
}

async fn positions_handler(
    State(state): State<ServerState>,
    Query(auth): Query<AuthRequest>,
) -> Result<Json<UsersResponse>, StatusCode> {
    info!("GET /positions");
    if !check_auth(&state, auth.password_hash.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let users = state.users.read().unwrap();
    let user_map: HashMap<String, ConnectedUser> = users
        .iter()
        .map(|(key, data)| (key.clone(), data.user.clone()))
        .collect();

    Ok(Json(UsersResponse { users: user_map }))
}

async fn update_position_handler(
    State(state): State<ServerState>,
    Json(update): Json<PositionUpdate>,
) -> Result<StatusCode, StatusCode> {
    info!("POST /update_position from {} at ¶{}-{}", update.name, update.position.start_element, update.position.end_element);
    if !check_auth(&state, update.password_hash.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let user_key = update.name.clone();
    
    let mut users = state.users.write().unwrap();
    users.insert(
        user_key,
        UserData {
            user: ConnectedUser {
                name: update.name,
                color: update.color,
                position: update.position,
            },
            last_heartbeat: Instant::now(),
        },
    );

    Ok(StatusCode::OK)
}

fn check_auth(state: &ServerState, provided_hash: Option<&str>) -> bool {
    match (&state.password_hash, provided_hash) {
        (None, _) => true,
        (Some(expected), Some(provided)) => expected == provided,
        (Some(_), None) => false,
    }
}

async fn heartbeat_cleanup(state: ServerState) {
    let mut interval = time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        
        let mut users = state.users.write().unwrap();
        let now = Instant::now();
        users.retain(|key, data| {
            let elapsed = now.duration_since(data.last_heartbeat);
            if elapsed > Duration::from_secs(10) {
                warn!("Removing inactive user: {}", key);
                false
            } else {
                true
            }
        });
    }
}

fn parse_epub(path: &PathBuf) -> Result<(Document, HashMap<String, Vec<u8>>)> {
    let mut doc = EpubDoc::new(path).context("Failed to open EPUB file")?;
    
    let title = doc.mdata("title").map(|m| m.value.clone());
    let language = doc.mdata("language").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());

    let metadata = DocumentMetadata {
        title,
        language,
        author,
    };

    let mut elements = Vec::new();
    let mut images = HashMap::new();

    let image_ids: Vec<String> = doc.resources
        .iter()
        .filter(|(_, resource)| resource.mime.starts_with("image/"))
        .map(|(id, _)| id.clone())
        .collect();

    for id in image_ids {
        if let Some((data, _mime)) = doc.get_resource(&id) {
            images.insert(id, data);
        }
    }

    for i in 0..doc.spine.len() {
        doc.set_current_chapter(i);
        
        if let Some((content, _mime)) = doc.get_current_str() {
            parse_html_content(&content, &mut elements);
        }
    }

    Ok((Document { metadata, elements }, images))
}

fn parse_html_content(html: &str, elements: &mut Vec<DocumentElement>) {
    let clean_text = strip_html_tags(html);
    let lines: Vec<&str> = clean_text.lines().collect();
    
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if is_likely_heading(trimmed) {
            elements.push(DocumentElement::Heading {
                content: trimmed.to_string(),
                level: 1,
            });
        } else {
            elements.push(DocumentElement::Text {
                content: trimmed.to_string(),
            });
        }
    }
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_script_or_style = false;
    let mut chars = html.chars().peekable();
    let mut tag_buffer = String::new();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            tag_buffer.clear();
            tag_buffer.push(ch);
            
            while let Some(&next_ch) = chars.peek() {
                chars.next();
                tag_buffer.push(next_ch);
                if next_ch == '>' {
                    break;
                }
            }

            let tag_lower = tag_buffer.to_lowercase();
            if tag_lower.contains("<script") || tag_lower.contains("<style") {
                in_script_or_style = true;
            } else if tag_lower.contains("</script") || tag_lower.contains("</style") {
                in_script_or_style = false;
            }
        } else if !in_script_or_style {
            result.push(ch);
        }
    }

    result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn is_likely_heading(text: &str) -> bool {
    if text.len() > 100 {
        return false;
    }

    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() > 15 {
        return false;
    }

    let uppercase_count = text.chars().filter(|c| c.is_uppercase()).count();
    let alpha_count = text.chars().filter(|c| c.is_alphabetic()).count();
    
    if alpha_count > 0 && (uppercase_count as f32 / alpha_count as f32) > 0.3 {
        return true;
    }

    if text.starts_with("Chapter") || text.starts_with("CHAPTER") {
        return true;
    }

    false
}
