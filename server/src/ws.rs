use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LogMessage {
    pub timestamp: String,
    pub r#type: String,
    pub status: u16,
    pub method: String,
    pub path: String,
    pub project_id: String,
    pub message: String,
}
