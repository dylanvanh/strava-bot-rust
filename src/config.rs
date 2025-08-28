use std::env::{self, VarError};

#[derive(Debug, Clone)]
pub struct Config {
    pub strava_client_id: String,
    pub strava_client_secret: String,
    pub strava_refresh_token: String,
}

impl Config {
    pub fn from_env() -> Result<Self, VarError> {
        Ok(Self {
            strava_client_id: env::var("STRAVA_CLIENT_ID")?,
            strava_client_secret: env::var("STRAVA_CLIENT_SECRET")?,
            strava_refresh_token: env::var("STRAVA_INITIAL_REFRESH_TOKEN")?,
        })
    }
}
