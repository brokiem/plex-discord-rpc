use crate::domain::models::{AppError, AppResult, MediaType, PlayerState, Session};
use crate::domain::traits::DiscordClient;
use discord_presence_rs::{
    activities::{Activity, ActivityType, Assets, StatusDisplayType, Timestamps},
    discord_connection::Client as DiscordRpc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DISCORD_APP_ID: &str = "1464540148707496009";
const MAX_RECONNECT_ATTEMPTS: u32 = 3;

pub struct DiscordPresenceClient {
    client: Option<DiscordRpc>,
    last_connection_attempt: Option<SystemTime>,
    reconnect_delay: Duration,
}

impl DiscordPresenceClient {
    pub fn new() -> Self {
        Self {
            client: None,
            last_connection_attempt: None,
            reconnect_delay: Duration::from_secs(2),
        }
    }

    /// Force reconnect by dropping current client and creating a new one
    fn force_reconnect(&mut self) -> AppResult<()> {
        // Drop existing client
        self.client = None;

        // Try to establish new connection
        self.establish_connection()
    }

    /// Establish a new connection with retry logic
    fn establish_connection(&mut self) -> AppResult<()> {
        // Check if we should wait before reconnecting
        if let Some(last_attempt) = self.last_connection_attempt {
            if let Ok(elapsed) = SystemTime::now().duration_since(last_attempt) {
                if elapsed < self.reconnect_delay {
                    return Err(AppError::DiscordRpc(
                        "Too soon to reconnect, waiting for cooldown".to_string(),
                    ));
                }
            }
        }

        self.last_connection_attempt = Some(SystemTime::now());

        // Try to create client with retries
        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            match DiscordRpc::new(DISCORD_APP_ID) {
                Ok(mut client) => {
                    // Try to handshake/initialize by setting a minimal activity
                    // This ensures the connection is actually established
                    let init_activity = Activity::new().set_activity_type(ActivityType::Watching);
                    if let Err(e) = client.set_activity(init_activity) {
                        log::warn!("Discord handshake failed on attempt {}: {:?}", attempt, e);
                        if attempt < MAX_RECONNECT_ATTEMPTS {
                            std::thread::sleep(Duration::from_millis(500 * attempt as u64));
                            continue;
                        }
                        return Err(AppError::DiscordRpc(format!(
                            "Failed to initialize Discord connection after {} attempts: {:?}",
                            MAX_RECONNECT_ATTEMPTS, e
                        )));
                    }

                    log::info!("Successfully connected to Discord on attempt {}", attempt);
                    self.client = Some(client);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Discord connection attempt {} failed: {:?}", attempt, e);
                    if attempt < MAX_RECONNECT_ATTEMPTS {
                        std::thread::sleep(Duration::from_millis(500 * attempt as u64));
                    } else {
                        return Err(AppError::DiscordRpc(format!(
                            "Failed to connect to Discord after {} attempts: {:?}. Is Discord running?",
                            MAX_RECONNECT_ATTEMPTS, e
                        )));
                    }
                }
            }
        }

        Err(AppError::DiscordRpc(
            "Failed to establish connection".to_string(),
        ))
    }
}

impl DiscordClient for DiscordPresenceClient {
    fn connect(&mut self) -> AppResult<()> {
        if self.client.is_none() {
            self.establish_connection()?;
        }
        Ok(())
    }

    fn update_presence(&mut self, session: &Session) -> AppResult<()> {
        // Ensure we have a connection
        if self.client.is_none() {
            log::info!("No Discord client, attempting to connect...");
            self.connect()?;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let (details, state, status_display, activity_type) = match session.media_type {
            MediaType::Episode => {
                let d = if let (Some(parent_idx), Some(idx)) =
                    (session.media_parent_index, session.media_index)
                {
                    format!("S{} · E{} — {}", parent_idx, idx, session.media_title)
                } else {
                    session.media_title.clone()
                };
                (
                    d,
                    session.media_grandparent_title.clone().unwrap_or_default(),
                    StatusDisplayType::State,
                    ActivityType::Watching,
                )
            }
            MediaType::Movie => (
                session.media_title.clone(),
                String::new(),
                StatusDisplayType::Details,
                ActivityType::Watching,
            ),
            MediaType::Track => (
                session.media_title.clone(),
                session.media_grandparent_title.clone().unwrap_or_default(), // Artist
                StatusDisplayType::State,
                ActivityType::Listening,
            ),
            _ => {
                // Generic fallback matching GenericSessionRenderer.cs
                // Details: Grandparent - Parent
                // State: Title
                let mut d = String::new();
                if let Some(gp) = &session.media_grandparent_title {
                    d.push_str(gp);
                }
                if let Some(p) = &session.media_parent_title {
                    if !d.is_empty() {
                        d.push_str(" - ");
                    }
                    d.push_str(p);
                }

                (
                    d,
                    session.media_title.clone(),
                    StatusDisplayType::Name, // Reference uses Name for generic
                    ActivityType::Watching,
                )
            }
        };

        // Note: Reference uses session.Thumbnail URL for large image.
        let large_image_url = session.thumbnail.clone().unwrap_or_else(|| "".to_string()); // Default to empty if no URL, image key fallback logic below

        let mut assets = Assets::new();

        if !large_image_url.is_empty() {
            // Use set_large_url for URLs as provided by the crate
            assets = assets.set_large_url(large_image_url.clone());
            assets = assets.set_large_text(session.media_title.clone());
        }

        match session.player_state {
            PlayerState::Paused => {
                assets = assets.set_small_image("pause-circle".to_string());
                assets = assets.set_small_text("Paused".to_string());
            }
            PlayerState::Buffering => {
                assets = assets.set_small_image("sand-clock".to_string());
                assets = assets.set_small_text("Buffering".to_string());
            }
            PlayerState::Idle => {
                assets = assets.set_small_image("sleep-mode".to_string());
                assets = assets.set_small_text("Idle".to_string());
            }
            _ => {
                // Playing: no small image usually
            }
        }

        log::debug!(
            "Updating Presence: Type={:?}, Details='{}', State='{}'",
            activity_type,
            details,
            state
        );

        let mut activity = Activity::new()
            .set_activity_type(activity_type)
            .set_status_display_type(status_display) // Restored
            .set_assets(assets);

        if !details.is_empty() {
            activity = activity.set_details(details);
        }

        if !state.is_empty() {
            activity = activity.set_state(state);
        }

        if session.player_state == PlayerState::Playing {
            let elapsed_secs = session.view_offset / 1000;
            let remaining_secs = (session.duration.saturating_sub(session.view_offset)) / 1000;
            let start = now.saturating_sub(elapsed_secs);
            let end = now.saturating_add(remaining_secs);

            activity = activity.set_timestamps(Timestamps::new().set_start(start).set_end(end));
        }

        // Try to set activity, with reconnection on failure
        if let Some(client) = &mut self.client {
            if let Err(e) = client.set_activity(activity.clone()) {
                log::warn!("Failed to set activity, attempting reconnection: {:?}", e);

                // Try to reconnect once
                if let Err(reconnect_err) = self.force_reconnect() {
                    return Err(AppError::DiscordRpc(format!(
                        "Failed to set activity and reconnection failed: {:?}",
                        reconnect_err
                    )));
                }

                // Try setting activity again with new connection
                if let Some(client) = &mut self.client {
                    client.set_activity(activity).map_err(|e| {
                        AppError::DiscordRpc(format!(
                            "Failed to set activity after reconnect: {:?}",
                            e
                        ))
                    })?;
                } else {
                    return Err(AppError::DiscordRpc(
                        "Client disconnected after reconnection attempt".to_string(),
                    ));
                }
            }
        } else {
            return Err(AppError::DiscordRpc("No client available".to_string()));
        }

        Ok(())
    }

    fn clear_presence(&mut self) -> AppResult<()> {
        if let Some(client) = &mut self.client {
            // Send empty activity to clear rich presence
            let activity = Activity::new().set_activity_type(ActivityType::Watching);

            if let Err(e) = client.set_activity(activity.clone()) {
                log::warn!("Failed to clear activity, attempting reconnection: {:?}", e);

                // Try to reconnect and clear again
                if self.force_reconnect().is_ok() {
                    if let Some(client) = &mut self.client {
                        client.set_activity(activity).map_err(|e| {
                            AppError::DiscordRpc(format!(
                                "Failed to clear activity after reconnect: {:?}",
                                e
                            ))
                        })?;
                    }
                }
                // If reconnect fails, we still return Ok since clearing is not critical
            }
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.client.is_some()
    }
}
