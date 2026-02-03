use crate::domain::models::*;
use crate::domain::traits::{DiscordClient, PlexClient};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct MonitorService {
    plex_client: Arc<dyn PlexClient>,
    discord_client: Arc<Mutex<dyn DiscordClient>>,
    last_session: Option<Session>,
    idle_since: Option<Instant>,
    notification_rx: Option<tokio::sync::mpsc::Receiver<()>>,
    last_update_time: Option<Instant>,
}

impl MonitorService {
    pub fn new(
        plex_client: Arc<dyn PlexClient>,
        discord_client: Arc<Mutex<dyn DiscordClient>>,
    ) -> Self {
        Self {
            plex_client,
            discord_client,
            last_session: None,
            idle_since: None,
            notification_rx: None,
            last_update_time: None,
        }
    }

    pub async fn clear_state(&mut self) -> AppResult<()> {
        let mut discord = self.discord_client.lock().await;
        // Ignore error on clear, we just want to try
        let _ = discord.clear_presence();
        self.last_session = None;
        self.idle_since = None;
        self.notification_rx = None;
        self.last_update_time = None;
        Ok(())
    }

    pub async fn update(&mut self, config: &AppConfig) -> AppResult<String> {
        if !config.is_authenticated() {
            let mut discord = self.discord_client.lock().await;
            let _ = discord.clear_presence();
            return Ok("Not authenticated".to_string());
        }

        let token = config.auth_token.as_ref().unwrap();
        let username = config.username.as_ref().unwrap();

        let server_address = match &config.server_address {
            Some(a) => a,
            None => {
                let mut discord = self.discord_client.lock().await;
                let _ = discord.clear_presence();
                return Ok("No server selected".to_string());
            }
        };

        let server_port = match config.server_port {
            Some(p) => p,
            None => {
                let mut discord = self.discord_client.lock().await;
                let _ = discord.clear_presence();
                return Ok("No server selected".to_string());
            }
        };

        let server = PlexServer {
            name: config.server_name.clone().unwrap_or_default(),
            address: server_address.clone(),
            port: server_port,
            owned: config.is_owned.unwrap_or(false),
        };

        // IMPORTANT: Try to connect to Discord early if not already connected
        {
            let mut discord = self.discord_client.lock().await;
            if !discord.is_connected() {
                if let Err(e) = discord.connect() {
                    log::warn!("Discord connection failed, will retry later: {:?}", e);
                    // Don't fail the whole update, just log and continue
                }
            }
        }

        // Decision: Should we poll?
        let should_poll = if server.owned {
            // Owned servers: polling is reliable and standard
            true
        } else {
            // Shared servers: use WebSocket to avoid ban/spam
            if self.notification_rx.is_none() {
                // Try to connect
                match self
                    .plex_client
                    .listen_for_notifications(&server, token, username)
                    .await
                {
                    Ok(rx) => {
                        self.notification_rx = Some(rx);
                        // Poll immediately on connect
                        true
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to Plex notification socket: {}. Falling back to polling.", e);
                        // If WS fails, fallback to polling
                        true
                    }
                }
            } else {
                // Check if we received notification
                let mut received = false;
                if let Some(rx) = &mut self.notification_rx {
                    loop {
                        match rx.try_recv() {
                            Ok(_) => received = true,
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                eprintln!(
                                    "Plex notification socket disconnected. triggering reconnect."
                                );
                                self.notification_rx = None;
                                received = true;
                                break;
                            }
                        }
                    }
                }

                // If we just reset rx to None, we want to try reconnect immediately?
                // The next update call will handle reconnection since rx is None.
                // But we returned `received = true` so effective_poll becomes true, which is fine,
                // it will try to fetch sessions which might fail if network is down, but that's handled.
                received
            }
        };

        // Also poll if we have no last session (initial load) or periodic update is needed?
        // Actually, for shared servers, if we just connected, we return true.
        // If we have a session playing, we might want to poll to update viewOffset drifting anyway?
        // Spec says: "Emit only on change (compare all fields, 5s viewOffset drift)"
        // If we rely purely on WS for shared, we might miss progress updates if they don't send socket events for every second.
        // Plex WS sends "playing" notification periodically? No, usually state changes.
        // But for "Watching", we need timestamps. Timestamps are calculated once.
        // So we only need to update if player state changes or media changes.
        // So WS trigger is sufficient.

        // However, user might have just selected the server.
        // Let's poll if `last_session` is None to ensure we get initial state.
        let effective_poll = should_poll || self.last_session.is_none();

        if !effective_poll {
            // Just return last status or "Monitoring (Shared)"
            if let Some(s) = &self.last_session {
                return Ok(format!("Playing: {}", s.media_title)); // Approximation
            } else {
                return Ok("Monitoring (Idle)".to_string());
            }
        }

        let fetch_result = self
            .plex_client
            .get_sessions(&server, token, username)
            .await;

        // Treat Idle state as no active session
        let active_session = match fetch_result {
            Ok(Some(session)) if session.player_state == PlayerState::Idle => None,
            Ok(other) => other,
            Err(e) => return Err(e),
        };

        match active_session {
            Some(session) => {
                // Active session found, reset idle timer
                self.idle_since = None;

                let mut status_msg = format!("Playing: {}", session.media_title);

                // Check if session changed or needs update
                // Simple logic: always update discord if playing, or if state changed, etc.
                // Optimally we diff.
                let should_update = if let Some(last) = &self.last_session {
                    let mut changed = last.media_title != session.media_title
                        || last.player_state != session.player_state
                        || last.media_type != session.media_type;

                    // Check for seek (drift) if playing
                    if !changed && session.player_state == PlayerState::Playing {
                        if let Some(last_time) = self.last_update_time {
                            let elapsed_ms = last_time.elapsed().as_millis() as u64;
                            let expected_offset = last.view_offset + elapsed_ms;
                            // Use 3s threshold (3000ms) to allow minor network jitter but catch seeks
                            let drift = if session.view_offset > expected_offset {
                                session.view_offset - expected_offset
                            } else {
                                expected_offset - session.view_offset
                            };

                            if drift > 3000 {
                                changed = true;
                                log::debug!(
                                    "Detected seek: drift {}ms (expected {}, got {})",
                                    drift,
                                    expected_offset,
                                    session.view_offset
                                );
                            }
                        }
                    }
                    changed
                } else {
                    true
                };

                if should_update {
                    let mut discord = self.discord_client.lock().await;
                    // Ensure connected
                    if !discord.is_connected() {
                        let _ = discord.connect(); // Try connect, ignore error for now log internally
                    }
                    if let Err(e) = discord.update_presence(&session) {
                        return Ok(format!("Failed to update Discord: {:?}", e));
                    }
                }

                match session.player_state {
                    PlayerState::Paused => status_msg = format!("Paused: {}", session.media_title),
                    PlayerState::Buffering => {
                        status_msg = format!("Buffering: {}", session.media_title)
                    }
                    _ => {}
                }

                self.last_session = Some(session);
                self.last_update_time = Some(Instant::now());
                Ok(status_msg)
            }
            None => {
                if self.last_session.is_some() {
                    match self.idle_since {
                        None => {
                            // First time realizing we are idle, start timer
                            self.idle_since = Some(Instant::now());
                            Ok("Waiting for idle debounce...".to_string())
                        }
                        Some(start_time) => {
                            if start_time.elapsed() > Duration::from_secs(3) {
                                // Debounce passed, clear presence
                                let mut discord = self.discord_client.lock().await;
                                let _ = discord.clear_presence();
                                self.last_session = None;
                                self.idle_since = None;
                                Ok("No active session".to_string())
                            } else {
                                Ok("Waiting for idle debounce...".to_string())
                            }
                        }
                    }
                } else {
                    Ok("No active session".to_string())
                }
            }
        }
    }

    pub async fn get_servers(&self, config: &AppConfig) -> AppResult<Vec<PlexServer>> {
        if let Some(token) = &config.auth_token {
            self.plex_client.get_servers(token).await
        } else {
            Err(AppError::Auth("Not authenticated".into()))
        }
    }

    pub fn get_last_session(&self) -> Option<&Session> {
        self.last_session.as_ref()
    }
}
