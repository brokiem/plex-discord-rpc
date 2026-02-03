use crate::application::auth_service::AuthService;
use crate::application::monitor_service::MonitorService;
use crate::domain::models::*;
use crate::domain::traits::ConfigStore;
use eframe::egui;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

// --- Enums for Async Communication ---
pub enum AppMessage {
    AuthStarted,
    AuthUrlReady(OAuthPinInfo),
    AuthCheckResult(Option<PlexAuth>),
    AuthFailed(String),
    ServersFetched(Vec<PlexServer>),
    ServersFetchFailed(String),
    VerificationStarted,
    VerificationSuccess(String),
    VerificationFailed(String),
    MonitorUpdate(String),
    MonitorError(String),
    ConfigSaved,
    ConfigSaveFailed(String),
}

// --- Notification System ---
#[derive(Clone)]
struct Notification {
    message: String,
    kind: NotificationKind,
    created_at: Instant,
    ttl: Duration,
}

#[derive(Clone, PartialEq)]
enum NotificationKind {
    Info,
    Success,
    Error,
}

// --- Activity Details ---
#[derive(Clone, Default)]
struct ActivityInfo {
    status: String,
    last_update: Option<Instant>,
    is_playing: bool,
    is_paused: bool,
}

// --- Main App Struct ---
pub struct PlexDiscordApp {
    // Services
    auth_service: Arc<AuthService>,
    monitor_service: Arc<tokio::sync::Mutex<MonitorService>>,
    config_store: Arc<dyn ConfigStore>,

    // State
    config: AppConfig,
    app_state: ApplicationState,
    oauth_info: Option<OAuthPinInfo>,
    servers: Vec<PlexServer>,

    // UI State & feedback
    is_loading_servers: bool,
    is_checking_auth: bool,
    is_verifying: bool,
    notifications: VecDeque<Notification>,
    activity_info: ActivityInfo,

    // Manual Connection Input
    custom_server_ip: String,
    custom_server_port: String,
    custom_server_owned: bool,

    // Async Runtime & Communication
    rt: Runtime,
    tx: mpsc::Sender<AppMessage>,
    rx: mpsc::Receiver<AppMessage>,

    // Tickers
    last_oauth_poll: Instant,
    last_monitor_tick: Instant,
}

impl PlexDiscordApp {
    pub fn new(
        auth_service: Arc<AuthService>,
        monitor_service: Arc<tokio::sync::Mutex<MonitorService>>,
        config_store: Arc<dyn ConfigStore>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let config = config_store.load().unwrap_or_default();

        // Determine initial state based on config
        let app_state = if config.is_authenticated() {
            if config.server_address.is_some() {
                ApplicationState::Verifying // Start with verification
            } else {
                ApplicationState::ServerSelection
            }
        } else {
            ApplicationState::Login
        };

        let mut app = Self {
            auth_service,
            monitor_service,
            config_store,
            config,
            app_state,
            oauth_info: None,
            servers: Vec::new(),
            is_loading_servers: false,
            is_checking_auth: false,
            is_verifying: false,
            notifications: VecDeque::new(),
            activity_info: ActivityInfo::default(),
            custom_server_ip: String::new(),
            custom_server_port: "32400".to_string(),
            custom_server_owned: true,
            rt: Runtime::new().unwrap(),
            tx,
            rx,
            last_oauth_poll: Instant::now(),
            last_monitor_tick: Instant::now(),
        };

        // If we start in ServerSelection, fetch servers immediately
        if app.app_state == ApplicationState::ServerSelection {
            app.dispatch_fetch_servers();
        }

        // If we start in Verifying, verify the connection
        if app.app_state == ApplicationState::Verifying {
            app.dispatch_verify_connection();
        }

        app
    }

    // --- Action Dispatchers (Spawn Async Tasks) ---

    fn dispatch_save_config(&self) {
        let tx = self.tx.clone();
        let store = self.config_store.clone();
        let config = self.config.clone();

        self.rt.spawn(async move {
            match store.save(&config) {
                Ok(_) => tx.send(AppMessage::ConfigSaved).ok(),
                Err(e) => tx
                    .send(AppMessage::ConfigSaveFailed(format!("{:?}", e)))
                    .ok(),
            };
        });
    }

    fn dispatch_start_login(&self) {
        let tx = self.tx.clone();
        let service = self.auth_service.clone();

        tx.send(AppMessage::AuthStarted).ok();

        self.rt.spawn(async move {
            match service.start_login().await {
                Ok(info) => tx.send(AppMessage::AuthUrlReady(info)).ok(),
                Err(e) => tx.send(AppMessage::AuthFailed(format!("{:?}", e))).ok(),
            };
        });
    }

    fn dispatch_check_auth(&mut self) {
        if self.last_oauth_poll.elapsed() < Duration::from_secs(2) || self.is_checking_auth {
            return;
        }
        self.last_oauth_poll = Instant::now();

        if let Some(info) = &self.oauth_info {
            self.is_checking_auth = true;
            let tx = self.tx.clone();
            let service = self.auth_service.clone();
            let pin_id = info.pin_id;

            self.rt.spawn(async move {
                match service.check_auth_status(pin_id).await {
                    Ok(auth_opt) => tx.send(AppMessage::AuthCheckResult(auth_opt)).ok(),
                    Err(e) => tx
                        .send(AppMessage::AuthFailed(format!("Check error: {:?}", e)))
                        .ok(),
                };
            });
        }
    }

    fn dispatch_fetch_servers(&mut self) {
        if self.is_loading_servers {
            return;
        }

        self.is_loading_servers = true;
        let tx = self.tx.clone();
        let monitor = self.monitor_service.clone();
        let config = self.config.clone();

        self.rt.spawn(async move {
            let service = monitor.lock().await;
            match service.get_servers(&config).await {
                Ok(servers) => tx.send(AppMessage::ServersFetched(servers)).ok(),
                Err(e) => tx
                    .send(AppMessage::ServersFetchFailed(format!("{:?}", e)))
                    .ok(),
            };
        });
    }

    fn dispatch_verify_connection(&mut self) {
        if self.is_verifying {
            return;
        }

        self.is_verifying = true;
        let tx = self.tx.clone();
        let monitor = self.monitor_service.clone();
        let config = self.config.clone();

        tx.send(AppMessage::VerificationStarted).ok();

        self.rt.spawn(async move {
            let mut service = monitor.lock().await;
            match service.update(&config).await {
                Ok(msg) => tx.send(AppMessage::VerificationSuccess(msg)).ok(),
                Err(e) => tx
                    .send(AppMessage::VerificationFailed(format!("{:?}", e)))
                    .ok(),
            };
        });
    }

    fn dispatch_monitor_tick(&mut self) {
        if self.last_monitor_tick.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.last_monitor_tick = Instant::now();

        let tx = self.tx.clone();
        let monitor = self.monitor_service.clone();
        let config = self.config.clone();

        self.rt.spawn(async move {
            let mut service = monitor.lock().await;
            match service.update(&config).await {
                Ok(msg) => tx.send(AppMessage::MonitorUpdate(msg)).ok(),
                Err(e) => tx.send(AppMessage::MonitorError(format!("{:?}", e))).ok(),
            };
        });
    }

    // --- Helper Methods ---

    fn add_notification(&mut self, message: String, kind: NotificationKind) {
        self.notifications.push_back(Notification {
            message,
            kind,
            created_at: Instant::now(),
            ttl: Duration::from_secs(4),
        });
        if self.notifications.len() > 5 {
            self.notifications.pop_front();
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMessage::AuthStarted => {
                    self.add_notification(
                        "Connecting to Plex...".to_string(),
                        NotificationKind::Info,
                    );
                }
                AppMessage::AuthUrlReady(info) => {
                    let _ = webbrowser::open(&info.auth_url);
                    self.oauth_info = Some(info);
                    self.app_state = ApplicationState::WaitingForAuth;
                    self.add_notification(
                        "Browser opened. Please approve access.".to_string(),
                        NotificationKind::Success,
                    );
                }
                AppMessage::AuthCheckResult(Some(auth)) => {
                    self.is_checking_auth = false;
                    self.config.auth_token = Some(auth.auth_token);
                    self.config.username = Some(auth.username);
                    self.dispatch_save_config();
                    self.app_state = ApplicationState::ServerSelection;
                    self.oauth_info = None;
                    self.add_notification(
                        "Authentication Successful!".to_string(),
                        NotificationKind::Success,
                    );
                    self.dispatch_fetch_servers();
                }
                AppMessage::AuthCheckResult(None) => {
                    self.is_checking_auth = false;
                }
                AppMessage::AuthFailed(e) => {
                    self.is_checking_auth = false;
                    self.add_notification(e, NotificationKind::Error);
                }
                AppMessage::ServersFetched(servers) => {
                    self.is_loading_servers = false;
                    self.servers = servers;
                    if self.servers.is_empty() {
                        self.add_notification(
                            "No servers found associated with account.".to_string(),
                            NotificationKind::Info,
                        );
                    } else {
                        self.add_notification(
                            format!("Found {} servers.", self.servers.len()),
                            NotificationKind::Success,
                        );
                    }
                }
                AppMessage::ServersFetchFailed(e) => {
                    self.is_loading_servers = false;
                    self.add_notification(
                        format!("Failed to load servers: {}", e),
                        NotificationKind::Error,
                    );
                }
                AppMessage::VerificationStarted => {
                    self.add_notification(
                        "Verifying server connection...".to_string(),
                        NotificationKind::Info,
                    );
                }
                AppMessage::VerificationSuccess(msg) => {
                    self.is_verifying = false;
                    self.dispatch_save_config();
                    self.app_state = ApplicationState::Running;
                    self.activity_info.status = msg;
                    self.activity_info.last_update = Some(Instant::now());
                    self.add_notification(
                        "Connected to server successfully!".to_string(),
                        NotificationKind::Success,
                    );
                }
                AppMessage::VerificationFailed(e) => {
                    self.is_verifying = false;
                    self.app_state = ApplicationState::ServerSelection;
                    // Clear server config since it failed
                    self.config.server_address = None;
                    self.config.server_port = None;
                    self.config.server_name = None;
                    self.add_notification(
                        format!("Server verification failed: {}", e),
                        NotificationKind::Error,
                    );
                }
                AppMessage::MonitorUpdate(msg) => {
                    self.activity_info.last_update = Some(Instant::now());
                    self.activity_info.is_playing = msg.contains("Playing");
                    self.activity_info.is_paused = msg.contains("Paused");
                    self.activity_info.status = msg;
                }
                AppMessage::MonitorError(e) => {
                    log::warn!("Monitor error: {}", e);
                    self.activity_info.status = format!("Error: Connection issue");
                }
                AppMessage::ConfigSaved => {}
                AppMessage::ConfigSaveFailed(e) => {
                    self.add_notification(
                        format!("Could not save settings: {}", e),
                        NotificationKind::Error,
                    );
                }
            }
        }
    }
}

// --- UI Implementation ---
impl eframe::App for PlexDiscordApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Process Async Messages
        self.handle_messages();

        // 2. Background Logic Ticks
        match self.app_state {
            ApplicationState::WaitingForAuth => {
                self.dispatch_check_auth();
                ctx.request_repaint_after(Duration::from_millis(500));
            }
            ApplicationState::Verifying => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            ApplicationState::Running => {
                self.dispatch_monitor_tick();
                ctx.request_repaint_after(Duration::from_millis(1000));
            }
            _ => {}
        }

        // 3. UI Styling
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
        ctx.set_style(style);

        // 4. Notifications Overlay
        self.render_notifications(ctx);

        // 5. Header
        self.render_header(ctx);

        // 6. Main Content
        egui::CentralPanel::default().show(ctx, |ui| match self.app_state {
            ApplicationState::Login => self.ui_login(ui),
            ApplicationState::WaitingForAuth => self.ui_waiting_auth(ui),
            ApplicationState::ServerSelection => self.ui_server_selection(ui),
            ApplicationState::Running => self.ui_dashboard(ui),
            ApplicationState::Verifying => self.ui_verifying(ui),
        });
    }
}

impl PlexDiscordApp {
    // --- UI Sections ---

    fn render_header(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("🎬 Plex Discord RPC").strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(user) = &self.config.username {
                        ui.label(
                            egui::RichText::new(format!("👤 {}", user))
                                .strong()
                                .color(ui.visuals().text_color()),
                        );
                    } else {
                        ui.label(egui::RichText::new("Not Logged In").weak());
                    }

                    if self.app_state == ApplicationState::Running {
                        ui.separator();
                        // Use actual circle character
                        ui.colored_label(egui::Color32::from_rgb(76, 175, 80), "Active");
                    }

                    if self.is_loading_servers || self.is_checking_auth || self.is_verifying {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(4.0);
        });
    }

    fn render_notifications(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.notifications
            .retain(|n| now.duration_since(n.created_at) < n.ttl);

        if self.notifications.is_empty() {
            return;
        }

        let window_rect = ctx.available_rect();
        let mut y_offset = window_rect.max.y - 20.0;

        for notification in self.notifications.iter().rev() {
            let (bg_color, border_color) = match notification.kind {
                NotificationKind::Info => (
                    egui::Color32::from_rgba_unmultiplied(40, 80, 120, 230),
                    egui::Color32::from_rgb(70, 130, 180),
                ),
                NotificationKind::Success => (
                    egui::Color32::from_rgba_unmultiplied(30, 80, 40, 230),
                    egui::Color32::from_rgb(76, 175, 80),
                ),
                NotificationKind::Error => (
                    egui::Color32::from_rgba_unmultiplied(100, 30, 30, 230),
                    egui::Color32::from_rgb(200, 60, 60),
                ),
            };

            egui::Area::new(egui::Id::new(format!(
                "toast_{:?}",
                notification.created_at
            )))
            .anchor(
                egui::Align2::RIGHT_BOTTOM,
                egui::vec2(-10.0, y_offset - window_rect.max.y),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(bg_color)
                    .stroke(egui::Stroke::new(1.0, border_color))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let icon = match notification.kind {
                                NotificationKind::Info => "ℹ",
                                NotificationKind::Success => "✅",
                                NotificationKind::Error => "⚠️",
                            };
                            ui.label(icon);
                            ui.label(
                                egui::RichText::new(&notification.message)
                                    .color(egui::Color32::WHITE),
                            );
                        });
                    });
            });

            y_offset -= 40.0;
        }
    }

    fn ui_login(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Welcome Back");
                ui.add_space(10.0);
                ui.label("Connect your Plex account to display your media status on Discord.");
                ui.add_space(20.0);

                let btn = egui::Button::new("Login with Plex").min_size(egui::vec2(120.0, 40.0));

                if ui.add(btn).clicked() {
                    self.dispatch_start_login();
                }
            });
        });
    }

    fn ui_waiting_auth(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.add_space(20.0);
                ui.heading("Waiting for Authorization");
                ui.label("A browser window has opened. Please verify the login.");

                if let Some(info) = self.oauth_info.clone() {
                    ui.add_space(20.0);
                    ui.group(|ui| {
                        ui.label("Is the browser not opening?");
                        ui.monospace(format!("Code: {}", info.code));
                        if ui.button("Copy Link to Clipboard").clicked() {
                            ui.ctx().copy_text(info.auth_url.clone());
                            self.add_notification("Link copied".into(), NotificationKind::Success);
                        }
                    });
                }

                ui.add_space(30.0);
                if ui.button("Cancel").clicked() {
                    self.oauth_info = None;
                    self.app_state = ApplicationState::Login;
                }
            });
        });
    }

    fn ui_verifying(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.add(egui::Spinner::new().size(40.0));
                ui.add_space(20.0);
                ui.heading("Verifying Connection");

                if let Some(name) = &self.config.server_name {
                    ui.label(format!("Connecting to: {}", name));
                }

                if let Some(addr) = &self.config.server_address {
                    if let Some(port) = self.config.server_port {
                        ui.label(
                            egui::RichText::new(format!("{}:{}", addr, port))
                                .weak()
                                .small(),
                        );
                    }
                }

                ui.add_space(30.0);
                if ui.button("Cancel").clicked() {
                    self.is_verifying = false;
                    self.app_state = ApplicationState::ServerSelection;
                    self.config.server_address = None;
                }
            });
        });
    }

    fn ui_server_selection(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Select Server");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🚪 Logout").clicked() {
                    self.dispatch_disconnect();
                    self.config = AppConfig::default();
                    self.dispatch_save_config();
                    self.app_state = ApplicationState::Login;
                }
                if ui
                    .button(if self.is_loading_servers {
                        "⏳ Fetching..."
                    } else {
                        "🔄 Refresh"
                    })
                    .clicked()
                {
                    self.dispatch_fetch_servers();
                }
            });
        });

        ui.separator();
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::CollapsingHeader::new("🔧 Manual Connection")
                .default_open(false)
                .show(ui, |ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        egui::Grid::new("manual_grid")
                            .num_columns(2)
                            .spacing([10.0, 10.0])
                            .show(ui, |ui| {
                                ui.label("Host / IP:");
                                ui.text_edit_singleline(&mut self.custom_server_ip)
                                    .on_hover_text("e.g. 192.168.1.50");
                                ui.end_row();

                                ui.label("Port:");
                                ui.text_edit_singleline(&mut self.custom_server_port);
                                ui.end_row();
                            });

                        ui.checkbox(
                            &mut self.custom_server_owned,
                            "I own this server (enables polling)",
                        );
                        ui.add_space(5.0);

                        if ui.button("Connect via IP").clicked() {
                            if self.custom_server_ip.is_empty() {
                                self.add_notification(
                                    "IP Address is required".into(),
                                    NotificationKind::Error,
                                );
                            } else {
                                match self.custom_server_port.parse::<u16>() {
                                    Ok(port) => {
                                        self.connect_to_server_manual(
                                            self.custom_server_ip.clone(),
                                            port,
                                            self.custom_server_owned,
                                        );
                                    }
                                    Err(_) => self.add_notification(
                                        "Invalid Port Number".into(),
                                        NotificationKind::Error,
                                    ),
                                }
                            }
                        }
                    });
                });

            ui.add_space(20.0);
            ui.label(egui::RichText::new("Discovered Servers").strong());

            if self.is_loading_servers && self.servers.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.spinner();
                    ui.label("Searching for Plex servers...");
                });
            } else if self.servers.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label("Server list is empty.");
                    ui.label(egui::RichText::new("Try refreshing the server list.").small());
                });
            } else {
                let servers = self.servers.clone();
                for (idx, server) in servers.iter().enumerate() {
                    self.render_server_card(ui, idx, server);
                }
            }
        });
    }

    fn render_server_card(&mut self, ui: &mut egui::Ui, idx: usize, server: &PlexServer) {
        egui::Frame::group(ui.style())
            .inner_margin(12.0)
            .corner_radius(6)
            .fill(ui.style().visuals.faint_bg_color)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🖥").size(24.0));

                    ui.vertical(|ui| {
                        ui.heading(&server.name);
                        ui.horizontal(|ui| {
                            ui.label(format!("{}:{}", server.address, server.port));
                            if server.owned {
                                ui.colored_label(egui::Color32::from_rgb(100, 255, 100), "Owned");
                            } else {
                                ui.colored_label(egui::Color32::from_rgb(255, 200, 100), "Shared");
                            }
                        });
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Connect").clicked() {
                            self.connect_to_server_auto(idx);
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }

    fn ui_dashboard(&mut self, ui: &mut egui::Ui) {
        let server_name = self
            .config
            .server_name
            .clone()
            .unwrap_or("Unknown Server".to_string());
        let server_addr = self
            .config
            .server_address
            .clone()
            .unwrap_or("Unknown Address".to_string());
        let server_port = self.config.server_port.unwrap_or(32400);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // --- Connection Status Card ---
            egui::Frame::group(ui.style())
                .inner_margin(16.0)
                .corner_radius(8)
                .fill(ui.style().visuals.faint_bg_color)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("🌐").size(32.0));
                        ui.vertical(|ui| {
                            ui.heading(&server_name);
                            ui.label(
                                egui::RichText::new(format!("{}:{}", server_addr, server_port))
                                    .monospace()
                                    .weak(),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.colored_label(egui::Color32::from_rgb(76, 175, 80), "Connected");
                        });
                    });
                });

            // --- Activity Monitor Card ---
            egui::Frame::group(ui.style())
                .inner_margin(16.0)
                .corner_radius(8)
                .fill(ui.style().visuals.faint_bg_color)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal(|ui| {
                        ui.heading("Activity Monitor");

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(last_update) = self.activity_info.last_update {
                                let elapsed = last_update.elapsed().as_secs();
                                ui.label(
                                    egui::RichText::new(format!("Updated {}s ago", elapsed))
                                        .small()
                                        .weak(),
                                );
                            }
                        });
                    });

                    ui.separator();
                    ui.add_space(8.0);

                    // Status display with icon
                    ui.horizontal(|ui| {
                        let (status_icon, status_color) = if self.activity_info.is_playing {
                            ("▶", egui::Color32::from_rgb(76, 175, 80))
                        } else if self.activity_info.is_paused {
                            ("⏸", egui::Color32::from_rgb(255, 193, 7))
                        } else {
                            ("⏹", ui.visuals().text_color())
                        };

                        ui.label(egui::RichText::new(status_icon).size(20.0));
                        ui.colored_label(
                            status_color,
                            egui::RichText::new(&self.activity_info.status).size(15.0),
                        );
                    });

                    ui.add_space(8.0);

                    // Additional info
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Server Type:").weak().small());
                        if let Some(owned) = self.config.is_owned {
                            if owned {
                                ui.label(egui::RichText::new("Owned (Polling)").small());
                            } else {
                                ui.label(egui::RichText::new("Shared (WebSocket)").small());
                            }
                        }
                    });
                });

            ui.add_space(20.0);

            // --- Disconnect Button ---
            ui.vertical_centered(|ui| {
                if ui
                    .add(
                        egui::Button::new("Disconnect Server")
                            .fill(egui::Color32::from_rgb(80, 40, 40))
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(150.0, 35.0)),
                    )
                    .clicked()
                {
                    self.dispatch_disconnect();
                }
            });

            ui.add_space(20.0);
        });
    }

    // --- State Transitions ---

    fn connect_to_server_auto(&mut self, idx: usize) {
        if let Some(server) = self.servers.get(idx) {
            self.config.server_name = Some(server.name.clone());
            self.config.server_address = Some(server.address.clone());
            self.config.server_port = Some(server.port);
            self.config.is_owned = Some(server.owned);
            self.app_state = ApplicationState::Verifying;
            self.dispatch_verify_connection();
        }
    }

    fn connect_to_server_manual(&mut self, ip: String, port: u16, owned: bool) {
        self.config.server_name = Some("Custom Server".to_string());
        self.config.server_address = Some(ip);
        self.config.server_port = Some(port);
        self.config.is_owned = Some(owned);
        self.app_state = ApplicationState::Verifying;
        self.dispatch_verify_connection();
    }

    fn dispatch_disconnect(&mut self) {
        let monitor = self.monitor_service.clone();
        let _config = self.config.clone();
        self.rt.spawn(async move {
            let mut service = monitor.lock().await;
            let _ = service.clear_state().await;
        });

        self.app_state = ApplicationState::ServerSelection;
        self.config.server_address = None;
        self.config.server_port = None;
        self.config.server_name = None;
        self.activity_info = ActivityInfo::default();
        self.dispatch_save_config();
        self.add_notification("Disconnected".into(), NotificationKind::Info);
    }
}
