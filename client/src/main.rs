use eframe::egui;
use epaint::{text::{LayoutJob, TextFormat}, Color32, FontFamily, FontId};
use shared::{Document, DocumentElement};
use tokio::runtime::Runtime;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Friend Reader",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(ReaderApp::new()))
        }),
    )
}

#[derive(Clone)]
struct LoginInfo {
    server_ip: String,
    server_port: String,
    display_name: String,
    user_color: Color32,
    password: String,
}

impl Default for LoginInfo {
    fn default() -> Self {
        Self {
            server_ip: "localhost".to_string(),
            server_port: "15470".to_string(),
            display_name: String::new(),
            user_color: Color32::from_rgb(100, 150, 255),
            password: String::new(),
        }
    }
}

enum AppState {
    Login(LoginInfo),
    Loading,
    Reader(ReaderState),
    Error(String),
}

struct ReaderApp {
    runtime: Runtime,
    state: AppState,
}

struct ReaderState {
    _server_url: String,
    user_name: String,
    user_color: String,
    password_hash: Option<String>,
    document: Document,
    scroll_offset: f32,
    desired_content_width: f32,
    last_layout_width: f32,
    laid_out_elements: Vec<LaidOutElement>,
    options_open: bool,
    users_open: bool,
    selected_font_family: FontFamily,
    font_size: f32,
    paragraph_spacing: f32,
    foreground_color: Color32,
    background_color: Color32,
    previous_font_family: FontFamily,
    previous_font_size: f32,
    previous_paragraph_spacing: f32,
    dragging_width_adjuster: bool,
    dragging_minimap: bool,
    anchor_element_index: Option<usize>,
    other_users: HashMap<String, shared::ConnectedUser>,
    following_user: Option<String>,
    last_users_fetch: Option<std::time::Instant>,
    last_position_update: Option<std::time::Instant>,
    last_sent_position: Option<(usize, usize)>,
}

use std::collections::HashMap;
use std::time::Instant as StdInstant;

#[derive(Clone)]
struct LaidOutElement {
    text: String,
    y_position: f32,
    height: f32,
}

impl ReaderApp {
    fn new() -> Self {
        Self {
            runtime: Runtime::new().unwrap(),
            state: AppState::Login(LoginInfo::default()),
        }
    }

    fn attempt_connection(&mut self, login_info: LoginInfo) {
        let display_name = login_info.display_name.trim();
        if display_name.is_empty() {
            self.state = AppState::Error("Display name cannot be empty".to_string());
            return;
        }

        let server_url = format!("http://{}:{}", login_info.server_ip, login_info.server_port);
        let user_name = display_name.to_string();
        let user_color = format!("#{:02x}{:02x}{:02x}", 
            login_info.user_color.r(), 
            login_info.user_color.g(), 
            login_info.user_color.b()
        );
        
        let password_hash = if !login_info.password.is_empty() {
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(login_info.password.as_bytes());
            Some(hex::encode(hasher.finalize()))
        } else {
            None
        };
        
        self.state = AppState::Loading;

        let result = self.runtime.block_on(async {
            let client = reqwest::Client::new();
            
            let health_response = client
                .get(format!("{}/health", server_url))
                .send()
                .await?;

            if !health_response.status().is_success() {
                return Err(anyhow::anyhow!("Server health check failed: {}", health_response.status()));
            }

            let doc_response = client
                .get(format!("{}/document", server_url))
                .send()
                .await?;

            if !doc_response.status().is_success() {
                return Err(anyhow::anyhow!("Failed to load document: {}", doc_response.status()));
            }

            let doc: Document = doc_response.json().await?;
            Ok(doc)
        });

        match result {
            Ok(document) => {
                let initial_font_family = FontFamily::Name("Japanese".into());
                let initial_font_size = 18.0;
                let initial_paragraph_spacing = 10.0;
                self.state = AppState::Reader(ReaderState {
                    _server_url: server_url,
                    user_name,
                    user_color,
                    password_hash,
                    document,
                    scroll_offset: 0.0,
                    desired_content_width: 600.0,
                    last_layout_width: 0.0,
                    laid_out_elements: Vec::new(),
                    options_open: false,
                    users_open: false,
                    selected_font_family: initial_font_family.clone(),
                    font_size: initial_font_size,
                    paragraph_spacing: initial_paragraph_spacing,
                    foreground_color: Color32::BLACK,
                    background_color: Color32::WHITE,
                    previous_font_family: initial_font_family,
                    previous_font_size: initial_font_size,
                    previous_paragraph_spacing: initial_paragraph_spacing,
                    dragging_width_adjuster: false,
                    dragging_minimap: false,
                    anchor_element_index: None,
                    other_users: HashMap::new(),
                    following_user: None,
                    last_users_fetch: None,
                    last_position_update: None,
                    last_sent_position: None,
                });
            }
            Err(e) => {
                self.state = AppState::Error(format!("Connection failed: {}", e));
            }
        }
    }

}

fn calculate_luminance(color: Color32) -> f32 {
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;
    0.299 * r + 0.587 * g + 0.114 * b
}

fn get_ui_background(background: Color32) -> Color32 {
    let luminance = calculate_luminance(background);
    if luminance < 0.5 {
        Color32::from_gray(40)
    } else {
        Color32::from_gray(230)
    }
}

fn get_ui_text_color(background: Color32) -> Color32 {
    let luminance = calculate_luminance(background);
    if luminance < 0.5 {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

fn parse_hex_color(hex: &str) -> Option<Color32> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
    } else {
        None
    }
}

impl eframe::App for ReaderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut should_connect = None;
        let mut should_back_to_login = false;

        match &mut self.state {
            AppState::Login(login_info) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default().fill(Color32::from_gray(240)))
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() * 0.3);
                            
                            egui::Frame::default()
                                .fill(Color32::WHITE)
                                .stroke(egui::Stroke::new(1.0, Color32::from_gray(200)))
                                .inner_margin(30.0)
                                .rounding(10.0)
                                .show(ui, |ui| {
                                    ui.set_width(400.0);
                                    ui.vertical_centered(|ui| {
                                        ui.heading("Friend Reader");
                                        ui.add_space(20.0);

                                        ui.horizontal(|ui| {
                                            ui.label("Server IP:");
                                            ui.add(egui::TextEdit::singleline(&mut login_info.server_ip)
                                                .desired_width(200.0));
                                        });
                                        
                                        ui.add_space(8.0);

                                        ui.horizontal(|ui| {
                                            ui.label("Port:");
                                            ui.add(egui::TextEdit::singleline(&mut login_info.server_port)
                                                .desired_width(200.0));
                                        });
                                        
                                        ui.add_space(8.0);

                                        ui.horizontal(|ui| {
                                            ui.label("Display Name:");
                                            ui.add(egui::TextEdit::singleline(&mut login_info.display_name)
                                                .desired_width(200.0));
                                        });
                                        
                                        ui.add_space(8.0);

                                        ui.horizontal(|ui| {
                                            ui.label("Your Color:");
                                            egui::color_picker::color_edit_button_srgba(
                                                ui,
                                                &mut login_info.user_color,
                                                egui::color_picker::Alpha::Opaque,
                                            );
                                        });
                                        
                                        ui.add_space(8.0);

                                        ui.horizontal(|ui| {
                                            ui.label("Password (optional):");
                                            ui.add(egui::TextEdit::singleline(&mut login_info.password)
                                                .password(true)
                                                .desired_width(200.0));
                                        });

                                        ui.add_space(20.0);

                                        if ui.button("Connect").clicked() {
                                            should_connect = Some(login_info.clone());
                                        }
                                    });
                                });
                        });
                    });
            }

            AppState::Loading => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Connecting to server...");
                    });
                });
            }

            AppState::Error(error_msg) => {
                let error_text = error_msg.clone();
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.4);
                        ui.colored_label(Color32::RED, error_text);
                        ui.add_space(20.0);
                        if ui.button("Back to Login").clicked() {
                            should_back_to_login = true;
                        }
                    });
                });
            }

            AppState::Reader(reader_state) => {
                let available_rect = ctx.available_rect();
                
                let minimap_width = 90.0;
                let min_side_margin = 50.0;
                let max_available_for_content = available_rect.width() - minimap_width - (min_side_margin * 2.0);
                
                let content_width = reader_state.desired_content_width
                    .max(200.0)
                    .min(max_available_for_content);

                let ui_bg_color = get_ui_background(reader_state.background_color);
                let ui_text_color = get_ui_text_color(reader_state.background_color);

                let font_or_spacing_changed = reader_state.selected_font_family != reader_state.previous_font_family
                    || (reader_state.font_size - reader_state.previous_font_size).abs() > 0.1
                    || (reader_state.paragraph_spacing - reader_state.previous_paragraph_spacing).abs() > 0.1;

                if font_or_spacing_changed {
                    let center_y = reader_state.scroll_offset + (available_rect.height() / 2.0);
                    reader_state.anchor_element_index = reader_state.laid_out_elements.iter()
                        .position(|e| e.y_position + e.height > center_y);
                    
                    reader_state.laid_out_elements.clear();
                    reader_state.previous_font_family = reader_state.selected_font_family.clone();
                    reader_state.previous_font_size = reader_state.font_size;
                    reader_state.previous_paragraph_spacing = reader_state.paragraph_spacing;
                }

                let need_layout = reader_state.laid_out_elements.is_empty() 
                    || (content_width - reader_state.last_layout_width).abs() > 1.0;

                if need_layout {
                    let center_y = reader_state.scroll_offset + (available_rect.height() / 2.0);
                    if reader_state.anchor_element_index.is_none() {
                        reader_state.anchor_element_index = reader_state.laid_out_elements.iter()
                            .position(|e| e.y_position + e.height > center_y);
                    }

                    reader_state.last_layout_width = content_width;

                    let mut laid_out = Vec::new();
                    let mut current_y = 0.0;

                    let font_id = FontId::new(reader_state.font_size, reader_state.selected_font_family.clone());

                    for (_idx, element) in reader_state.document.elements.iter().enumerate() {
                        let (text, is_heading) = match element {
                            DocumentElement::Text { content } => (content.clone(), false),
                            DocumentElement::Heading { content, level } => {
                                (format!("[HEADING LEVEL {}] {}", level, content), true)
                            }
                            DocumentElement::Image { id, .. } => {
                                (format!("[IMAGE: {}]", id), false)
                            }
                        };

                        let mut job = LayoutJob::default();
                        job.text = text.clone();
                        job.wrap.max_width = content_width;
                        job.sections.push(epaint::text::LayoutSection {
                            leading_space: 0.0,
                            byte_range: 0..text.len(),
                            format: TextFormat {
                                font_id: font_id.clone(),
                                color: reader_state.foreground_color,
                                ..Default::default()
                            },
                        });

                        let galley = ctx.fonts(|fonts| fonts.layout_job(job));
                        let text_height = galley.size().y;

                        let spacing = if is_heading { 
                            reader_state.paragraph_spacing * 2.0 
                        } else { 
                            reader_state.paragraph_spacing 
                        };

                        laid_out.push(LaidOutElement {
                            text,
                            y_position: current_y,
                            height: text_height,
                        });

                        current_y += text_height + spacing;
                    }

                    reader_state.laid_out_elements = laid_out;

                    if let Some(anchor_idx) = reader_state.anchor_element_index {
                        if anchor_idx < reader_state.laid_out_elements.len() {
                            let anchor_y = reader_state.laid_out_elements[anchor_idx].y_position;
                            reader_state.scroll_offset = (anchor_y - available_rect.height() / 2.0).max(0.0);
                        }
                        reader_state.anchor_element_index = None;
                    }
                }

                let total_height: f32 = reader_state.laid_out_elements.iter()
                    .map(|e| e.height + reader_state.paragraph_spacing)
                    .sum();

                let current_element_idx = reader_state.laid_out_elements.iter()
                    .position(|e| e.y_position + e.height > reader_state.scroll_offset)
                    .unwrap_or(0);

                let viewport_height = available_rect.height();
                let view_end_y = reader_state.scroll_offset + viewport_height;
                
                let end_element_idx = reader_state.laid_out_elements.iter()
                    .position(|e| e.y_position + e.height > view_end_y)
                    .unwrap_or(reader_state.laid_out_elements.len().saturating_sub(1));

                let current_position = (current_element_idx, end_element_idx);
                let position_changed = reader_state.last_sent_position.map(|last| last != current_position).unwrap_or(true);
                let time_elapsed = reader_state.last_position_update.map(|t| t.elapsed().as_millis() >= 250).unwrap_or(true);
                
                if position_changed || time_elapsed {
                    let server_url = reader_state._server_url.clone();
                    let user_name = reader_state.user_name.clone();
                    let user_color = reader_state.user_color.clone();
                    let password_hash = reader_state.password_hash.clone();
                    
                    let position = shared::Position {
                        start_element: current_element_idx,
                        start_percent: 0.0,
                        end_element: end_element_idx,
                        end_percent: 1.0,
                    };
                    
                    let update = shared::PositionUpdate {
                        name: user_name,
                        color: user_color,
                        position,
                        password_hash,
                    };
                    
                    let _result = self.runtime.block_on(async {
                        let client = reqwest::Client::new();
                        client
                            .post(format!("{}/update_position", server_url))
                            .json(&update)
                            .send()
                            .await?;
                        Ok::<_, anyhow::Error>(())
                    });
                    
                    reader_state.last_position_update = Some(StdInstant::now());
                    reader_state.last_sent_position = Some(current_position);
                }

                let should_fetch_users = reader_state.last_users_fetch.map(|t| t.elapsed().as_millis() >= 250).unwrap_or(true);
                
                if should_fetch_users {
                    let server_url = reader_state._server_url.clone();
                    let result = self.runtime.block_on(async {
                        let client = reqwest::Client::new();
                        let response = client
                            .get(format!("{}/positions", server_url))
                            .send()
                            .await?;
                        
                        let users_response: shared::UsersResponse = response.json().await?;
                        Ok::<_, anyhow::Error>(users_response.users)
                    });
                    
                    if let Ok(users) = result {
                        reader_state.other_users = users;
                        reader_state.last_users_fetch = Some(StdInstant::now());
                    }
                }
                
                ctx.request_repaint_after(std::time::Duration::from_millis(250));

                if let Some(following) = &reader_state.following_user {
                    if let Some(followed_user) = reader_state.other_users.get(following) {
                        if let Some(mid_element) = reader_state.laid_out_elements.get(followed_user.position.start_element) {
                            let target_scroll = mid_element.y_position;
                            let current_scroll = reader_state.scroll_offset;
                            let distance = (target_scroll - current_scroll).abs();
                            
                            if distance > 2000.0 {
                                reader_state.scroll_offset = target_scroll;
                            } else {
                                let speed: f32 = if distance > 500.0 { 50.0 } else { 20.0 };
                                let delta = (target_scroll - current_scroll).signum() * speed.min(distance);
                                reader_state.scroll_offset += delta;
                            }
                            
                            ctx.request_repaint();
                        }
                    }
                }

                let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta.abs() > 0.1 {
                    reader_state.following_user = None;
                }
                
                reader_state.scroll_offset = (reader_state.scroll_offset - scroll_delta)
                    .max(0.0)
                    .min(total_height - available_rect.height() + 100.0);

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    reader_state.scroll_offset += 50.0;
                    reader_state.following_user = None;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    reader_state.scroll_offset -= 50.0;
                    reader_state.following_user = None;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                    reader_state.scroll_offset += available_rect.height() * 0.8;
                    reader_state.following_user = None;
                }

                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    reader_state.following_user = None;
                }

                egui::TopBottomPanel::top("options_bar")
                    .frame(egui::Frame::default().fill(ui_bg_color).inner_margin(5.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Options").clicked() {
                                reader_state.options_open = !reader_state.options_open;
                            }

                            if ui.button("Users").clicked() {
                                reader_state.users_open = !reader_state.users_open;
                            }

                            if reader_state.following_user.is_some() {
                                if ui.button("Stop Following").clicked() {
                                    reader_state.following_user = None;
                                }
                            }

                            if ui.button("Disconnect").clicked() {
                                should_back_to_login = true;
                            }

                            if let Some(title) = &reader_state.document.metadata.title {
                                ui.separator();
                                ui.colored_label(ui_text_color, title);
                            }

                            if let Some(following_name) = &reader_state.following_user {
                                ui.separator();
                                ui.colored_label(Color32::from_rgb(100, 150, 255), format!("Following: {}", following_name));
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.colored_label(ui_text_color, format!("¶ {}/{}", 
                                    current_element_idx + 1, 
                                    reader_state.document.elements.len()
                                ));
                            });
                        });
                    });

                if reader_state.options_open {
                    egui::Window::new("Options")
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            ui.label("Font Family:");
                            egui::ComboBox::from_label("")
                                .selected_text(match &reader_state.selected_font_family {
                                    FontFamily::Name(name) => name.as_ref(),
                                    _ => "Default",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut reader_state.selected_font_family,
                                        FontFamily::Name("Japanese".into()),
                                        "Japanese (Noto Sans JP)",
                                    );
                                    ui.selectable_value(
                                        &mut reader_state.selected_font_family,
                                        FontFamily::Name("Chinese".into()),
                                        "Chinese (Noto Sans SC)",
                                    );
                                    ui.selectable_value(
                                        &mut reader_state.selected_font_family,
                                        FontFamily::Name("English".into()),
                                        "English (Roboto)",
                                    );
                                });

                            ui.add_space(10.0);

                            ui.label("Font Size:");
                            ui.add(egui::Slider::new(&mut reader_state.font_size, 10.0..=32.0));

                            ui.add_space(10.0);

                            ui.label("Paragraph Spacing:");
                            ui.add(egui::Slider::new(&mut reader_state.paragraph_spacing, 0.0..=40.0));

                            ui.add_space(10.0);

                            ui.label("Foreground Color:");
                            egui::color_picker::color_edit_button_srgba(
                                ui,
                                &mut reader_state.foreground_color,
                                egui::color_picker::Alpha::Opaque,
                            );

                            ui.add_space(10.0);

                            ui.label("Background Color:");
                            egui::color_picker::color_edit_button_srgba(
                                ui,
                                &mut reader_state.background_color,
                                egui::color_picker::Alpha::Opaque,
                            );

                            ui.add_space(10.0);

                            if ui.button("Close").clicked() {
                                reader_state.options_open = false;
                            }
                        });
                }

                if reader_state.users_open {
                    egui::Window::new("Users")
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            ui.label("Connected Users:");
                            ui.separator();

                            let mut users_list: Vec<_> = reader_state.other_users.iter().collect();
                            users_list.sort_by(|a, b| a.0.cmp(b.0));

                            if users_list.is_empty() {
                                ui.label("No users connected");
                            } else {
                                for (user_key, user) in users_list {
                                    let user_color = parse_hex_color(&user.color).unwrap_or(Color32::from_rgb(100, 150, 255));
                                    
                                    let is_self = user.name == reader_state.user_name;
                                    let is_following = reader_state.following_user.as_ref() == Some(user_key);
                                    
                                    let button_text = if is_self {
                                        format!("{} (you) [¶{}-{}]", user.name, user.position.start_element + 1, user.position.end_element + 1)
                                    } else if is_following {
                                        format!("✓ {} (following) [¶{}-{}]", user.name, user.position.start_element + 1, user.position.end_element + 1)
                                    } else {
                                        format!("{} [¶{}-{}]", user.name, user.position.start_element + 1, user.position.end_element + 1)
                                    };

                                    ui.horizontal(|ui| {
                                        let color_rect = egui::Rect::from_min_size(
                                            ui.cursor().min,
                                            egui::vec2(20.0, 20.0),
                                        );
                                        ui.painter().rect_filled(color_rect, 3.0, user_color);
                                        ui.add_space(25.0);

                                        if ui.button(button_text).clicked() {
                                            if is_self {
                                                reader_state.following_user = None;
                                            } else if is_following {
                                                reader_state.following_user = None;
                                            } else {
                                                reader_state.following_user = Some(user_key.clone());
                                            }
                                        }
                                    });
                                }
                            }

                            ui.add_space(10.0);

                            if ui.button("Close").clicked() {
                                reader_state.users_open = false;
                            }
                        });
                }

                egui::SidePanel::right("minimap")
                    .exact_width(minimap_width)
                    .frame(egui::Frame::default().fill(ui_bg_color))
                    .show(ctx, |ui| {
                        let rect = ui.available_rect_before_wrap();
                        let painter = ui.painter();

                        let total_elements = reader_state.document.elements.len() as f32;
                        if total_elements == 0.0 {
                            return;
                        }

                        let mut all_users: Vec<(&String, &shared::ConnectedUser)> = reader_state.other_users.iter()
                            .filter(|(_, user)| user.name != reader_state.user_name)
                            .collect();
                        all_users.sort_by(|a, b| a.0.cmp(b.0));

                        let my_element = current_element_idx as f32;
                        let my_ratio = (my_element / total_elements).clamp(0.0, 1.0);
                        let my_y = rect.min.y + my_ratio * rect.height();

                        for (idx, (_user_key, user)) in all_users.iter().enumerate() {
                            let user_element = user.position.start_element as f32;
                            let user_ratio = (user_element / total_elements).clamp(0.0, 1.0);
                            let y_pos = rect.min.y + user_ratio * rect.height();

                            let x_offset = (idx % 4) as f32 * (rect.width() / 5.0);
                            let line_start_x = rect.min.x + x_offset;
                            let line_end_x = rect.min.x + x_offset + (rect.width() / 5.0);

                            let color_str = &user.color;
                            let user_color = parse_hex_color(color_str).unwrap_or(Color32::from_rgb(100, 150, 255));

                            painter.line_segment(
                                [egui::pos2(line_start_x, y_pos), egui::pos2(line_end_x, y_pos)],
                                egui::Stroke::new(2.0, user_color),
                            );

                            let triangle_size = 10.0;
                            let triangle_x = line_end_x + 3.0;
                            let triangle_points = vec![
                                egui::pos2(triangle_x, y_pos),
                                egui::pos2(triangle_x + triangle_size, y_pos - triangle_size / 2.0),
                                egui::pos2(triangle_x + triangle_size, y_pos + triangle_size / 2.0),
                            ];

                            painter.add(egui::epaint::Shape::convex_polygon(
                                triangle_points,
                                user_color,
                                egui::Stroke::new(1.0, user_color.linear_multiply(0.7)),
                            ));
                        }

                        let my_line_end_x = rect.max.x;
                        let my_line_start_x = my_line_end_x - (rect.width() / 5.0);
                        let my_color = parse_hex_color(&reader_state.user_color).unwrap_or(Color32::from_rgb(100, 200, 100));

                        painter.line_segment(
                            [egui::pos2(my_line_start_x, my_y), egui::pos2(my_line_end_x, my_y)],
                            egui::Stroke::new(3.0, my_color),
                        );

                        let my_triangle_size = 10.0;
                        let my_triangle_x = my_line_start_x - my_triangle_size - 3.0;
                        let my_triangle_points = vec![
                            egui::pos2(my_triangle_x + my_triangle_size, my_y),
                            egui::pos2(my_triangle_x, my_y - my_triangle_size / 2.0),
                            egui::pos2(my_triangle_x, my_y + my_triangle_size / 2.0),
                        ];

                        painter.add(egui::epaint::Shape::convex_polygon(
                            my_triangle_points,
                            my_color,
                            egui::Stroke::new(1.5, my_color.linear_multiply(0.7)),
                        ));

                        let minimap_response = ui.interact(rect, egui::Id::new("minimap_interact"), egui::Sense::click_and_drag());

                        if minimap_response.clicked() || minimap_response.dragged() {
                            if let Some(pointer_pos) = ctx.pointer_interact_pos() {
                                let click_ratio = ((pointer_pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
                                let target_element_idx = (click_ratio * total_elements) as usize;
                                
                                if target_element_idx < reader_state.laid_out_elements.len() {
                                    reader_state.scroll_offset = reader_state.laid_out_elements[target_element_idx].y_position;
                                    reader_state.following_user = None;
                                }
                            }
                        }

                        if minimap_response.dragged() {
                            reader_state.dragging_minimap = true;
                        }

                        if minimap_response.drag_stopped() {
                            reader_state.dragging_minimap = false;
                        }
                    });

                egui::CentralPanel::default()
                    .frame(egui::Frame::default().fill(reader_state.background_color))
                    .show(ctx, |ui| {
                        let painter = ui.painter();
                        let rect = ui.available_rect_before_wrap();

                        let left_margin = (rect.width() - content_width) / 2.0;
                        let text_left_edge = rect.min.x + left_margin;

                        let adjuster_x = text_left_edge - 20.0;
                        let adjuster_rect = egui::Rect::from_center_size(
                            egui::pos2(adjuster_x, rect.center().y),
                            egui::vec2(10.0, 60.0),
                        );

                        let adjuster_response = ui.interact(
                            adjuster_rect,
                            egui::Id::new("width_adjuster"),
                            egui::Sense::click_and_drag(),
                        );

                        let adjuster_color = if adjuster_response.dragged() {
                            reader_state.dragging_width_adjuster = true;
                            Color32::from_rgb(150, 180, 255)
                        } else if adjuster_response.hovered() {
                            Color32::from_rgb(120, 150, 200)
                        } else {
                            Color32::from_gray(100)
                        };

                        painter.rect_filled(adjuster_rect, 3.0, adjuster_color);

                        if adjuster_response.dragged() {
                            if let Some(pointer_pos) = ctx.pointer_interact_pos() {
                                let center_x = rect.center().x;
                                let distance_from_center = (pointer_pos.x - center_x).abs();
                                let new_width = (distance_from_center * 2.0).max(200.0).min(max_available_for_content);
                                reader_state.desired_content_width = new_width;
                            }
                        }

                        if adjuster_response.drag_stopped() {
                            reader_state.dragging_width_adjuster = false;
                        }

                        let font_id = FontId::new(reader_state.font_size, reader_state.selected_font_family.clone());

                        for element in &reader_state.laid_out_elements {
                            let element_y = element.y_position - reader_state.scroll_offset;
                            
                            if element_y + element.height < 0.0 {
                                continue;
                            }
                            if element_y > rect.height() {
                                break;
                            }

                            let mut job = LayoutJob::default();
                            job.text = element.text.clone();
                            job.wrap.max_width = content_width;
                            job.sections.push(epaint::text::LayoutSection {
                                leading_space: 0.0,
                                byte_range: 0..element.text.len(),
                                format: TextFormat {
                                    font_id: font_id.clone(),
                                    color: reader_state.foreground_color,
                                    ..Default::default()
                                },
                            });

                            let galley = ui.fonts(|fonts| fonts.layout_job(job));
                            
                            let text_pos = egui::pos2(
                                text_left_edge,
                                rect.min.y + element_y,
                            );

                            painter.galley(text_pos, galley, reader_state.foreground_color);
                        }

                        let text_right_edge = text_left_edge + content_width;
                        
                        let mut sorted_users: Vec<_> = reader_state.other_users.iter()
                            .filter(|(_, user)| user.name != reader_state.user_name)
                            .collect();
                        sorted_users.sort_by(|a, b| a.1.name.cmp(&b.1.name));
                        
                        for (user_idx, (_user_key, user)) in sorted_users.iter().enumerate() {
                            let start_idx = user.position.start_element;
                            let end_idx = user.position.end_element;

                            if start_idx >= reader_state.laid_out_elements.len() {
                                continue;
                            }

                            let end_idx_clamped = end_idx.min(reader_state.laid_out_elements.len() - 1);
                            
                            let start_element = &reader_state.laid_out_elements[start_idx];
                            let end_element = &reader_state.laid_out_elements[end_idx_clamped];

                            let start_y = start_element.y_position - reader_state.scroll_offset;
                            let end_y = end_element.y_position + end_element.height - reader_state.scroll_offset;

                            if end_y < 0.0 || start_y > rect.height() {
                                continue;
                            }

                            let visible_start_y = start_y.max(0.0);
                            let visible_end_y = end_y.min(rect.height());

                            if visible_start_y >= visible_end_y {
                                continue;
                            }

                            let user_color = parse_hex_color(&user.color).unwrap_or(Color32::from_rgb(100, 150, 255));
                            
                            let bar_width = 5.0;
                            let bar_spacing = 2.0;
                            let bar_x = text_right_edge + 10.0 + (user_idx as f32 * (bar_width + bar_spacing));

                            let shadow_rect = egui::Rect::from_min_max(
                                egui::pos2(bar_x + 2.0, rect.min.y + visible_start_y),
                                egui::pos2(bar_x + bar_width + 2.0, rect.min.y + visible_end_y),
                            );
                            painter.rect_filled(shadow_rect, 0.0, Color32::from_black_alpha(80));

                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(bar_x, rect.min.y + visible_start_y),
                                egui::pos2(bar_x + bar_width, rect.min.y + visible_end_y),
                            );
                            painter.rect_filled(bar_rect, 0.0, user_color);
                        }
                    });
            }
        }

        if let Some(login_info) = should_connect {
            self.attempt_connection(login_info);
        }

        if should_back_to_login {
            self.state = AppState::Login(LoginInfo::default());
        }
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "noto_sans_jp".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansJP-Regular.ttf"
        )).into(),
    );

    fonts.font_data.insert(
        "noto_sans_sc".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansSC-Regular.ttf"
        )).into(),
    );

    fonts.font_data.insert(
        "roboto".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Roboto-Regular.ttf"
        )).into(),
    );

    fonts
        .families
        .entry(egui::FontFamily::Name("Japanese".into()))
        .or_default()
        .insert(0, "noto_sans_jp".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Name("Chinese".into()))
        .or_default()
        .insert(0, "noto_sans_sc".to_owned());

    fonts
        .families
        .entry(egui::FontFamily::Name("English".into()))
        .or_default()
        .insert(0, "roboto".to_owned());

    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "noto_sans_jp".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(1, "noto_sans_sc".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(2, "roboto".to_owned());

    ctx.set_fonts(fonts);
}
