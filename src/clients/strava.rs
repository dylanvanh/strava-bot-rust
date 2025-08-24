use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    expires_in: u64,
    token_type: String,
}

#[derive(Debug)]
struct TokenState {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

pub struct StravaClient {
    http: Client,
    base: Url,
    client_id: String,
    client_secret: String,
    token: Arc<Mutex<TokenState>>,
}

#[derive(Debug, Deserialize)]
pub struct StravaActivitySummary {
    id: u64,
    name: String,
    #[serde(rename = "type")]
    activity_type: String,
    start_date: String,
    distance: u64,
    private: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateDetails {
    hide_from_home: Option<bool>,
    name: Option<String>,
    description: Option<String>,
    commute: Option<bool>,
    trainer: Option<bool>,
    sport_type: Option<String>,
    gear_id: Option<String>,
}

impl StravaClient {
    pub fn new(id: String, secret: String, initial_refresh_token: String) -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            base: Url::parse("https://www.strava.com/api/v3/")?,
            client_id: id,
            client_secret: secret,
            token: Arc::new(Mutex::new(TokenState {
                access_token: String::new(),
                refresh_token: initial_refresh_token,
                expires_at: 0,
            })),
        })
    }

    fn is_token_expired(&self) -> bool {
        let token_state = self.token.lock().unwrap();
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let five_minutes_buffer = 5 * 60;
        token_state.expires_at.saturating_sub(current_time) < five_minutes_buffer
    }

    pub async fn refresh_access_token(&self) -> anyhow::Result<()> {
        let refresh_token = {
            let token_state = self.token.lock().unwrap();
            token_state.refresh_token.clone()
        };

        println!("Refreshing Strava access token");

        let response = Client::new()
            .post("https://www.strava.com/oauth/token")
            .json(&json!({
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "refresh_token": refresh_token,
                "grant_type": "refresh_token"
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await?;

        let mut token_state = self.token.lock().unwrap();
        token_state.access_token = response.access_token;
        token_state.refresh_token = response.refresh_token;
        token_state.expires_at = response.expires_at;

        println!(
            "Token refreshed successfully, expires at: {}",
            response.expires_at
        );
        Ok(())
    }

    async fn get_valid_token(&self) -> anyhow::Result<String> {
        if self.is_token_expired() {
            self.refresh_access_token().await?;
        }

        let token_state = self.token.lock().unwrap();
        Ok(token_state.access_token.clone())
    }

    pub async fn get_all_activities(
        &self,
        page: u32,
        per_page: u32,
    ) -> anyhow::Result<Vec<StravaActivitySummary>> {
        let token = self.get_valid_token().await?;
        let url = self.base.join("athlete/activities")?;

        println!(
            "Getting activities - page: {}, per_page: {}",
            page, per_page
        );

        let activities = self
            .http
            .get(url)
            .bearer_auth(token)
            .query(&[
                ("page", page.to_string()),
                ("per_page", per_page.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<StravaActivitySummary>>()
            .await?;

        println!("Retrieved {} activities", activities.len());
        Ok(activities)
    }

    pub async fn update_activity(
        &self,
        activity_id: String,
        update_details: UpdateDetails,
    ) -> anyhow::Result<StravaActivitySummary> {
        let token = self.get_valid_token().await?;
        let url = self.base.join(&format!("activities/{}", activity_id))?;

        println!(
            "Making PUT request to /activities/{} with: {:?}",
            activity_id, update_details
        );

        let result = self
            .http
            .put(url)
            .bearer_auth(token)
            .json(&update_details)
            .send()
            .await?;

        if !result.status().is_success() {
            eprintln!(
                "Error updating activity {}: status={}, statusText={}",
                activity_id,
                result.status().as_u16(),
                result.status().canonical_reason().unwrap_or("Unknown")
            );
        }

        let activity = result
            .error_for_status()?
            .json::<StravaActivitySummary>()
            .await?;

        println!("Update successful for activity {}", activity_id);
        Ok(activity)
    }
}
