use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub elements: Vec<DocumentElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub language: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DocumentElement {
    #[serde(rename = "text")]
    Text { content: String },
    #[serde(rename = "heading")]
    Heading { content: String, level: u8 },
    #[serde(rename = "image")]
    Image { id: String, url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub start_element: usize,
    pub start_percent: f32,
    pub end_element: usize,
    pub end_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedUser {
    pub name: String,
    pub color: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
    pub name: String,
    pub color: String,
    pub position: Position,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsersResponse {
    pub users: HashMap<String, ConnectedUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub requires_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub password_hash: Option<String>,
}
