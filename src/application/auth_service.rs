use crate::domain::models::*;
use crate::domain::traits::PlexClient;
use std::sync::Arc;

pub struct AuthService {
    plex_client: Arc<dyn PlexClient>,
}

impl AuthService {
    pub fn new(plex_client: Arc<dyn PlexClient>) -> Self {
        Self { plex_client }
    }

    pub async fn start_login(&self) -> AppResult<OAuthPinInfo> {
        self.plex_client.start_oauth_flow().await
    }

    pub async fn check_auth_status(&self, pin_id: u64) -> AppResult<Option<PlexAuth>> {
        self.plex_client.check_oauth_status(pin_id).await
    }
}
