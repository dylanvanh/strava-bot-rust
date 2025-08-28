use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

const VIRTUAL_RIDE_ACTIVITY_TYPE: &str = "VirtualRide";
const BIKE_RIDE_ACTIVITY_TYPE: &str = "Ride";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
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
    processed_activities: Arc<Mutex<HashSet<u64>>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StravaActivitySummary {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub activity_type: String,
    pub start_date: String,
    pub distance: f64,
    pub private: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateDetails {
    hide_from_home: Option<bool>,
    name: Option<String>,
    description: Option<String>,
    commute: Option<bool>,
    trainer: Option<bool>,
    sport_type: Option<String>,
    gear_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityMatch {
    pub indoor_activity: ActivityInfo,
    pub virtual_ride: ActivityInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActivityInfo {
    pub id: u64,
    pub name: String,
    pub start_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CleanupResult {
    pub hidden: Vec<u64>,
    pub matches: Vec<ActivityMatch>,
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
            processed_activities: Arc::new(Mutex::new(HashSet::new())),
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

    async fn refresh_access_token(&self) -> anyhow::Result<()> {
        let refresh_token = {
            let token_state = self.token.lock().unwrap();
            token_state.refresh_token.clone()
        };

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

        Ok(activities)
    }

    pub async fn update_activity(
        &self,
        activity_id: String,
        update_details: UpdateDetails,
    ) -> anyhow::Result<StravaActivitySummary> {
        let token = self.get_valid_token().await?;
        let url = self.base.join(&format!("activities/{}", activity_id))?;

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

        Ok(activity)
    }

    pub async fn hide_duplicate_indoor_rides(&self) -> anyhow::Result<CleanupResult> {
        let all_activities = self.get_all_activities(1, 200).await?;

        let processed_set = {
            let processed = self.processed_activities.lock().unwrap();
            processed.clone()
        };

        let public_indoor_bike_activities: Vec<_> = all_activities
            .iter()
            .filter(|activity| {
                is_indoor_bike_activity(activity)
                    && !activity.private
                    && !processed_set.contains(&activity.id)
            })
            .cloned()
            .collect();

        let all_virtual_ride_activities: Vec<_> = all_activities
            .iter()
            .filter(|activity| activity.activity_type == VIRTUAL_RIDE_ACTIVITY_TYPE)
            .cloned()
            .collect();

        let mut hidden_activity_ids = Vec::new();
        let mut matched_activity_pairs = Vec::new();

        for indoor_bike_activity in public_indoor_bike_activities {
            if let Some(corresponding_virtual_ride) =
                all_virtual_ride_activities.iter().find(|virtual_ride| {
                    are_activities_within_one_hour(&indoor_bike_activity, virtual_ride)
                })
            {
                matched_activity_pairs.push(ActivityMatch {
                    indoor_activity: ActivityInfo {
                        id: indoor_bike_activity.id,
                        name: indoor_bike_activity.name.clone(),
                        start_date: indoor_bike_activity.start_date.clone(),
                    },
                    virtual_ride: ActivityInfo {
                        id: corresponding_virtual_ride.id,
                        name: corresponding_virtual_ride.name.clone(),
                        start_date: corresponding_virtual_ride.start_date.clone(),
                    },
                });

                self.update_activity(
                    indoor_bike_activity.id.to_string(),
                    UpdateDetails {
                        hide_from_home: Some(true),
                        ..Default::default()
                    },
                )
                .await?;

                {
                    let mut processed = self.processed_activities.lock().unwrap();
                    processed.insert(indoor_bike_activity.id);
                }

                hidden_activity_ids.push(indoor_bike_activity.id);
            }
        }

        Ok(CleanupResult {
            hidden: hidden_activity_ids,
            matches: matched_activity_pairs,
        })
    }
}

pub fn is_indoor_bike_activity(activity: &StravaActivitySummary) -> bool {
    let is_ride_type = activity.activity_type == BIKE_RIDE_ACTIVITY_TYPE;
    let is_zero_distance = activity.distance == 0.0;

    is_ride_type && is_zero_distance
}

pub fn are_activities_within_one_hour(
    first_activity: &StravaActivitySummary,
    second_activity: &StravaActivitySummary,
) -> bool {
    let first_activity_start = match DateTime::parse_from_rfc3339(&first_activity.start_date) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return false,
    };

    let second_activity_start = match DateTime::parse_from_rfc3339(&second_activity.start_date) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return false,
    };

    let time_difference = (first_activity_start - second_activity_start).abs();
    let one_hour = chrono::Duration::hours(1);

    time_difference <= one_hour
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_activity(
        id: u64,
        name: &str,
        activity_type: &str,
        start_date: &str,
        distance: f64,
        private: bool,
    ) -> StravaActivitySummary {
        StravaActivitySummary {
            id,
            name: name.to_string(),
            activity_type: activity_type.to_string(),
            start_date: start_date.to_string(),
            distance,
            private,
        }
    }

    #[test]
    fn test_is_indoor_bike_activity() {
        let indoor_bike =
            create_activity(1, "Indoor Bike", "Ride", "2025-01-01T10:00:00Z", 0.0, false);
        let outdoor_bike = create_activity(
            2,
            "Outdoor Bike",
            "Ride",
            "2025-01-01T10:00:00Z",
            25000.0,
            false,
        );
        let run = create_activity(3, "Run", "Run", "2025-01-01T10:00:00Z", 0.0, false);

        assert!(is_indoor_bike_activity(&indoor_bike));
        assert!(!is_indoor_bike_activity(&outdoor_bike));
        assert!(!is_indoor_bike_activity(&run));
    }

    #[test]
    fn test_are_activities_within_one_hour() {
        let base_time = "2025-01-01T10:00:00Z";
        let within_hour = "2025-01-01T10:30:00Z";
        let beyond_hour = "2025-01-01T11:30:00Z";
        let before_hour = "2025-01-01T09:30:00Z";

        let activity1 = create_activity(1, "Activity 1", "Ride", base_time, 0.0, false);
        let activity2 =
            create_activity(2, "Activity 2", "VirtualRide", within_hour, 25000.0, false);
        let activity3 =
            create_activity(3, "Activity 3", "VirtualRide", beyond_hour, 25000.0, false);
        let activity4 =
            create_activity(4, "Activity 4", "VirtualRide", before_hour, 25000.0, false);

        assert!(are_activities_within_one_hour(&activity1, &activity2));
        assert!(!are_activities_within_one_hour(&activity1, &activity3));
        assert!(are_activities_within_one_hour(&activity1, &activity4));
    }

    #[test]
    fn test_activities_exactly_one_hour_apart_are_within_range() {
        let base_time = "2025-01-01T10:00:00Z";
        let exactly_one_hour = "2025-01-01T11:00:00Z";

        let activity1 = create_activity(1, "Activity 1", "Ride", base_time, 0.0, false);
        let activity2 = create_activity(
            2,
            "Activity 2",
            "VirtualRide",
            exactly_one_hour,
            25000.0,
            false,
        );

        assert!(are_activities_within_one_hour(&activity1, &activity2));
    }

    #[test]
    fn test_are_activities_within_one_hour_invalid_dates() {
        let valid_time = "2025-01-01T10:00:00Z";
        let invalid_time = "not-a-date";

        let activity1 = create_activity(1, "Activity 1", "Ride", valid_time, 0.0, false);
        let activity2 =
            create_activity(2, "Activity 2", "VirtualRide", invalid_time, 25000.0, false);

        assert!(!are_activities_within_one_hour(&activity1, &activity2));
        assert!(!are_activities_within_one_hour(&activity2, &activity1));
    }

    #[test]
    fn test_cleanup_result_serialization() {
        let activity_info = ActivityInfo {
            id: 123,
            name: "Test Activity".to_string(),
            start_date: "2025-01-01T10:00:00Z".to_string(),
        };

        let activity_match = ActivityMatch {
            indoor_activity: activity_info.clone(),
            virtual_ride: activity_info,
        };

        let cleanup_result = CleanupResult {
            hidden: vec![123, 456],
            matches: vec![activity_match],
        };

        let json = serde_json::to_string(&cleanup_result).unwrap();
        let deserialized: CleanupResult = serde_json::from_str(&json).unwrap();

        assert_eq!(cleanup_result.hidden.len(), deserialized.hidden.len());
        assert_eq!(cleanup_result.matches.len(), deserialized.matches.len());
    }
}
