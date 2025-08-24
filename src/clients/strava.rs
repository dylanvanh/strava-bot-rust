//! Strava API client for handling authentication and API requests.
//!
//! This module provides a client for interacting with the Strava API v3,
//! including automatic token refresh and activity management.

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

/// Strava API client with automatic token refresh.
///
/// Handles OAuth2 token management and provides methods to interact
/// with the Strava API v3 endpoints.
pub struct StravaClient {
    http: Client,
    base: Url,
    client_id: String,
    client_secret: String,
    token: Arc<Mutex<TokenState>>,
}

/// Summary information for a Strava activity.
///
/// Contains the basic fields returned from the Strava API
/// when listing activities.
#[derive(Debug, Deserialize)]
pub struct StravaActivitySummary {
    /// Unique identifier for the activity
    id: u64,
    /// The name of the activity
    name: String,
    /// Type of activity (e.g., "Ride", "Run", "Swim")
    #[serde(rename = "type")]
    activity_type: String,
    /// ISO 8601 formatted date string
    start_date: String,
    /// Distance in meters
    distance: u64,
    /// Whether the activity is private
    private: bool,
}

/// Parameters for updating a Strava activity.
///
/// All fields are optional - only provided fields will be updated.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateDetails {
    /// Whether to hide this activity from the home feed
    hide_from_home: Option<bool>,
    /// The name of the activity
    name: Option<String>,
    /// Description of the activity
    description: Option<String>,
    /// Whether this activity is a commute
    commute: Option<bool>,
    /// Whether this activity was on a trainer
    trainer: Option<bool>,
    /// Sport type of the activity
    sport_type: Option<String>,
    /// Identifier for the gear used
    gear_id: Option<String>,
}

impl StravaClient {
    /// Creates a new Strava API client.
    ///
    /// # Arguments
    ///
    /// * `id` - Your Strava application's client ID
    /// * `secret` - Your Strava application's client secret
    /// * `initial_refresh_token` - A valid refresh token for OAuth2
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to build or the base URL is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// let client = StravaClient::new(
    ///     "client_id".to_string(),
    ///     "client_secret".to_string(),
    ///     "refresh_token".to_string()
    /// )?;
    /// ```
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

    /// Refreshes the OAuth2 access token using the refresh token.
    ///
    /// This is called automatically when the token is expired or about to expire.
    ///
    /// # Errors
    ///
    /// Returns an error if the token refresh request fails or returns invalid data.
    async fn refresh_access_token(&self) -> anyhow::Result<()> {
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

    /// Fetches a list of the authenticated athlete's activities.
    ///
    /// # Arguments
    ///
    /// * `page` - Page number (1-indexed)
    /// * `per_page` - Number of activities per page (max 200)
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or returns invalid data.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example(client: &StravaClient) -> anyhow::Result<()> {
    /// let activities = client.get_all_activities(1, 50).await?;
    /// for activity in activities {
    ///     println!("Activity: {}", activity.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
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

    /// Updates an existing Strava activity.
    ///
    /// # Arguments
    ///
    /// * `activity_id` - The ID of the activity to update
    /// * `update_details` - The fields to update (all fields are optional)
    ///
    /// # Errors
    ///
    /// Returns an error if the activity doesn't exist or the update fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example(client: &StravaClient) -> anyhow::Result<()> {
    /// let updates = UpdateDetails {
    ///     name: Some("Morning Run".to_string()),
    ///     description: Some("Great weather!".to_string()),
    ///     ..Default::default()
    /// };
    /// let updated = client.update_activity("12345".to_string(), updates).await?;
    /// # Ok(())
    /// # }
    /// ```
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
