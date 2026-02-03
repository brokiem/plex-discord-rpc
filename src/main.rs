mod application;
mod domain;
mod infrastructure;
mod presentation;

use crate::application::auth_service::AuthService;
use crate::application::monitor_service::MonitorService;
use crate::domain::traits::ConfigStore;
use crate::infrastructure::config_store::FileConfigStore;
use crate::infrastructure::discord_client::DiscordPresenceClient;
use crate::infrastructure::plex_client::ReqwestPlexClient;
use crate::presentation::ui::PlexDiscordApp;
use eframe::egui;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

fn main() -> eframe::Result<()> {
    env_logger::init();

    // Dependency Injection
    let config_store = Arc::new(FileConfigStore::new());
    let config = config_store.load().unwrap_or_default();

    // Use saved client ID or generate new one
    let client_id = if config.client_id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        config.client_id.clone()
    };

    let plex_client =
        Arc::new(ReqwestPlexClient::new(client_id.clone()).expect("Failed to create Plex Client"));
    let discord_client = Arc::new(Mutex::new(DiscordPresenceClient::new()));

    let auth_service = Arc::new(AuthService::new(plex_client.clone()));
    let monitor_service = Arc::new(Mutex::new(MonitorService::new(plex_client, discord_client)));

    // Create App
    let app = PlexDiscordApp::new(auth_service, monitor_service, config_store);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 500.0])
            .with_resizable(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Plex Discord RPC",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
