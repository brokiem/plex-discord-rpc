use crate::domain::models::AppError;
use crate::domain::models::AppResult;
use crate::domain::models::*;
use crate::domain::traits::PlexClient;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

const PLEX_TV_API: &str = "https://plex.tv/api/v2";

pub struct ReqwestPlexClient {
    client_id: String,
    http_client: Client,
}

impl ReqwestPlexClient {
    pub fn new(client_id: String) -> AppResult<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Config(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client_id,
            http_client,
        })
    }

    async fn send_with_retry(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> AppResult<reqwest::Response> {
        let mut retries = 0;
        const MAX_RETRIES: u32 = 3;
        const INITIAL_DELAY: u64 = 1; // seconds

        loop {
            // We need to clone the request builder because `send` consumes it.
            // However, RequestBuilder isn't easily cloneable if it has streams, but for simple GET/POST it is.
            // Wait, reqwest::RequestBuilder allows `try_clone()`.
            let request = request_builder.try_clone().ok_or_else(|| {
                AppError::PlexApi("Failed to clone request for retry strategy".to_string())
            })?; // This is a bit hacky, normally one would rebuild request.
                 // But for this simple app, try_clone should work for most simple requests.
                 // If try_clone fails (streaming body), we can't retry easily.

            match request.send().await {
                Ok(response) => {
                    if response.status().is_server_error() {
                        if retries >= MAX_RETRIES {
                            return Ok(response);
                        }
                    } else {
                        return Ok(response);
                    }
                }
                Err(e) => {
                    if retries >= MAX_RETRIES {
                        return Err(AppError::Network(e));
                    }
                }
            }

            retries += 1;
            tokio::time::sleep(Duration::from_secs(INITIAL_DELAY * retries as u64)).await;
        }
    }

    async fn get_username(&self, auth_token: &str) -> AppResult<String> {
        #[derive(Deserialize)]
        struct UserResponse {
            username: String,
        }

        let request = self
            .http_client
            .get("https://plex.tv/api/v2/user")
            .header("X-Plex-Token", auth_token)
            .header("X-Plex-Client-Identifier", &self.client_id)
            .header("X-Plex-Product", "Plex Discord RPC")
            .header("X-Plex-Version", "1.0.0")
            .header("Accept", "application/json");

        let response = self.send_with_retry(request).await?;

        if !response.status().is_success() {
            return Err(AppError::PlexApi(format!(
                "Failed to get username: {}",
                response.status()
            )));
        }

        let user: UserResponse = response.json().await.map_err(AppError::Network)?;
        Ok(user.username)
    }
}

#[async_trait]
impl PlexClient for ReqwestPlexClient {
    async fn start_oauth_flow(&self) -> AppResult<OAuthPinInfo> {
        #[derive(Deserialize)]
        struct PinResponse {
            id: u64,
            code: String,
        }

        let request = self
            .http_client
            .post("https://plex.tv/api/v2/pins")
            .header("X-Plex-Product", "Plex Discord RPC")
            .header("X-Plex-Client-Identifier", &self.client_id)
            .header("X-Plex-Version", "1.0.0")
            .header("Accept", "application/json")
            .query(&[("strong", "true")]);

        let response = self.send_with_retry(request).await?;

        if !response.status().is_success() {
            return Err(AppError::PlexApi(format!(
                "Failed to start OAuth: {}",
                response.status()
            )));
        }

        let pin: PinResponse = response.json().await.map_err(AppError::Network)?;

        Ok(OAuthPinInfo {
            pin_id: pin.id,
            code: pin.code.clone(),
            auth_url: format!("https://app.plex.tv/auth#?clientID={}&code={}&context[device][product]=Plex%20Discord%20RPC",
                              self.client_id, pin.code),
        })
    }

    async fn check_oauth_status(&self, pin_id: u64) -> AppResult<Option<PlexAuth>> {
        #[derive(Deserialize)]
        struct PinCheckResponse {
            #[serde(rename = "authToken")]
            auth_token: Option<String>,
        }

        let request = self
            .http_client
            .get(&format!("https://plex.tv/api/v2/pins/{}", pin_id))
            .header("X-Plex-Client-Identifier", &self.client_id)
            .header("X-Plex-Product", "Plex Discord RPC")
            .header("X-Plex-Version", "1.0.0")
            .header("Accept", "application/json");

        let response = self.send_with_retry(request).await?;

        if !response.status().is_success() {
            return Err(AppError::PlexApi(format!(
                "Failed to check PIN status: {}",
                response.status()
            )));
        }

        let pin_check: PinCheckResponse = response.json().await.map_err(AppError::Network)?;

        if let Some(token) = pin_check.auth_token {
            let username = self.get_username(&token).await?;
            Ok(Some(PlexAuth {
                auth_token: token,
                username,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_servers(&self, auth_token: &str) -> AppResult<Vec<PlexServer>> {
        #[derive(Deserialize)]
        struct Resource {
            name: String,
            owned: bool,
            connections: Vec<Connection>,
        }

        #[derive(Deserialize, Clone)]
        struct Connection {
            address: String,
            port: u16,
            local: bool,
        }

        let request = self
            .http_client
            .get(&format!("{}/resources", PLEX_TV_API))
            .header("X-Plex-Token", auth_token)
            .header("X-Plex-Client-Identifier", &self.client_id)
            .header("X-Plex-Product", "Plex Discord RPC")
            .header("X-Plex-Version", "1.0.0")
            .header("Accept", "application/json")
            .query(&[("includeHttps", "1"), ("includeRelay", "1")]);

        let response = self.send_with_retry(request).await?;

        if !response.status().is_success() {
            return Err(AppError::PlexApi(format!(
                "Failed to get servers: {}",
                response.status()
            )));
        }

        let resources: Vec<Resource> = response.json().await.map_err(AppError::Network)?;

        let servers = resources
            .into_iter()
            .filter_map(|r| {
                let connection = r
                    .connections
                    .iter()
                    .find(|c| c.local)
                    .or_else(|| r.connections.first())
                    .cloned()?;

                Some(PlexServer {
                    name: r.name,
                    address: connection.address,
                    port: connection.port,
                    owned: r.owned,
                })
            })
            .collect();

        Ok(servers)
    }

    async fn get_sessions(
        &self,
        server: &PlexServer,
        auth_token: &str,
        username: &str,
    ) -> AppResult<Option<Session>> {
        #[derive(Deserialize)]
        struct MediaContainer {
            #[serde(rename = "Metadata", default)]
            metadata: Vec<Metadata>,
        }

        #[derive(Deserialize)]
        struct Metadata {
            #[serde(rename = "type")]
            media_type: String,
            title: String,
            #[serde(default)]
            index: Option<u32>,
            #[serde(rename = "parentTitle", default)]
            parent_title: Option<String>,
            #[serde(rename = "parentIndex", default)]
            parent_index: Option<u32>,
            #[serde(rename = "grandparentTitle", default)]
            grandparent_title: Option<String>,
            duration: u64,
            #[serde(rename = "viewOffset")]
            view_offset: u64,
            #[serde(default)]
            thumb: Option<String>,
            #[serde(rename = "grandparentThumb", default)]
            grandparent_thumb: Option<String>,
            #[serde(rename = "Player")]
            player: Player,
            #[serde(rename = "User")]
            user: User,
        }

        #[derive(Deserialize)]
        struct Player {
            state: String,
        }

        #[derive(Deserialize)]
        struct User {
            title: String,
        }

        let url = format!("http://{}:{}/status/sessions", server.address, server.port);

        let request = self
            .http_client
            .get(&url)
            .header("X-Plex-Token", auth_token)
            .header("X-Plex-Client-Identifier", &self.client_id)
            .header("X-Plex-Product", "Plex Discord RPC")
            .header("X-Plex-Version", "1.0.0")
            .header("Accept", "application/json");

        let response = self.send_with_retry(request).await?;

        if !response.status().is_success() {
            // Often server might be unreachable or unauthorized, handle gracefully?
            // For now return error so UI updates
            return Err(AppError::PlexApi(format!(
                "Failed to get sessions: {}",
                response.status()
            )));
        }

        #[derive(Deserialize)]
        struct Response {
            #[serde(rename = "MediaContainer")]
            media_container: MediaContainer,
        }

        let data: Response = response.json().await.map_err(AppError::Network)?;

        let user_sessions: Vec<_> = data
            .media_container
            .metadata
            .into_iter()
            .filter(|m| m.user.title == username)
            .collect();

        let session = user_sessions
            .iter()
            .find(|m| m.player.state == "playing")
            .or_else(|| user_sessions.iter().find(|m| m.player.state == "buffering"))
            .or_else(|| user_sessions.iter().find(|m| m.player.state == "paused"));

        if let Some(s) = session {
            let thumbnail = s.thumb.as_ref().or(s.grandparent_thumb.as_ref()).map(|t| {
                format!(
                    "http://{}:{}/{}?X-Plex-Token={}",
                    server.address,
                    server.port,
                    t.trim_start_matches('/'),
                    auth_token
                )
            });

            Ok(Some(Session {
                media_title: s.title.clone(),
                media_index: s.index,
                media_parent_title: s.parent_title.clone(),
                media_parent_index: s.parent_index,
                media_grandparent_title: s.grandparent_title.clone(),
                player_state: match s.player.state.as_str() {
                    "playing" => PlayerState::Playing,
                    "paused" => PlayerState::Paused,
                    "buffering" => PlayerState::Buffering,
                    _ => PlayerState::Idle,
                },
                media_type: match s.media_type.as_str() {
                    "episode" => MediaType::Episode,
                    "movie" => MediaType::Movie,
                    "track" => MediaType::Track,
                    _ => MediaType::Unknown,
                },
                duration: s.duration,
                view_offset: s.view_offset,
                thumbnail,
            }))
        } else {
            Ok(None)
        }
    }

    async fn listen_for_notifications(
        &self,
        server: &PlexServer,
        auth_token: &str,
        _username: &str, // Just notification trigger, application layer will refetch/filter
    ) -> AppResult<tokio::sync::mpsc::Receiver<()>> {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let ws_url = format!(
            "ws://{}:{}/:/websockets/notifications?X-Plex-Token={}",
            server.address, server.port, auth_token
        );

        // Connect
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| AppError::PlexApi(format!("WebSocket connection failed: {}", e)))?;

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        tokio::spawn(async move {
            let (_, mut read) = ws_stream.split();

            while let Some(msg) = read.next().await {
                if let Ok(msg) = msg {
                    if msg.is_text() || msg.is_binary() {
                        // Parse? For now just signal "something happened"
                        // In shared server mode, any notification is worth checking session status
                        // because shared server sessions endpoint might rely on notifications or
                        // effectively we just want to know when state changes.
                        //
                        // Real implementation would parse `NotificationContainer` to filter "playing" events.
                        // Given "reliable" request, let's just debounce signal in monitor loop and trigger session fetch.
                        //
                        // If we send signal on every message, we might spam.
                        // Let's assume sending signal is cheap.
                        if tx.send(()).await.is_err() {
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        });

        Ok(rx)
    }
}
