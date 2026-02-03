use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Plex API error: {0}")]
    PlexApi(String),
    #[error("Discord RPC error: {0}")]
    DiscordRpc(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Authentication error: {0}")]
    Auth(String),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub auth_token: Option<String>,
    pub username: Option<String>,
    pub client_id: String,
    pub server_address: Option<String>,
    pub server_port: Option<u16>,
    pub server_name: Option<String>,
    pub is_owned: Option<bool>,
}

impl AppConfig {
    pub fn is_authenticated(&self) -> bool {
        self.auth_token.is_some() && self.username.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexAuth {
    pub auth_token: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct OAuthPinInfo {
    pub pin_id: u64,
    pub code: String,
    pub auth_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlexServer {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub owned: bool,
}

impl std::fmt::Display for PlexServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}:{})", self.name, self.address, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerState {
    Playing,
    Paused,
    Buffering,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MediaType {
    Episode,
    Movie,
    Track,
    Unknown,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub media_title: String,
    pub media_index: Option<u32>,
    pub media_parent_title: Option<String>,
    pub media_parent_index: Option<u32>,
    pub media_grandparent_title: Option<String>,
    pub player_state: PlayerState,
    pub media_type: MediaType,
    pub duration: u64,
    pub view_offset: u64,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationState {
    Login,
    WaitingForAuth,
    ServerSelection,
    Verifying,
    Running,
}
