use crate::domain::models::*;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;

#[cfg_attr(test, automock)]
#[async_trait]
pub trait PlexClient: Send + Sync {
    async fn start_oauth_flow(&self) -> AppResult<OAuthPinInfo>;
    async fn check_oauth_status(&self, pin_id: u64) -> AppResult<Option<PlexAuth>>;
    async fn get_servers(&self, auth_token: &str) -> AppResult<Vec<PlexServer>>;
    async fn get_sessions(
        &self,
        server: &PlexServer,
        auth_token: &str,
        username: &str,
    ) -> AppResult<Option<Session>>;

    // Returns a stream or receiver. For simplicity effectively, just a receiver via a callback or channel?
    // Let's return a tokio::sync::mpsc::Receiver<()>. Signaling "something changed".
    // Or just a loop helper?
    // "Listen" implies streams.
    async fn listen_for_notifications(
        &self,
        server: &PlexServer,
        auth_token: &str,
        username: &str,
    ) -> AppResult<tokio::sync::mpsc::Receiver<()>>;
}

#[cfg_attr(test, automock)]
pub trait DiscordClient: Send + Sync {
    fn connect(&mut self) -> AppResult<()>;
    fn update_presence(&mut self, session: &Session) -> AppResult<()>;
    fn clear_presence(&mut self) -> AppResult<()>;
    fn is_connected(&self) -> bool;
}

#[cfg_attr(test, automock)]
pub trait ConfigStore: Send + Sync {
    fn load(&self) -> AppResult<AppConfig>;
    fn save(&self, config: &AppConfig) -> AppResult<()>;
}
