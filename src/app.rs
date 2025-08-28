use crate::clients::strava::StravaClient;
use crate::config::Config;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct App {
    strava_client: Arc<StravaClient>,
    scheduler: JobScheduler,
}

impl App {
    pub async fn new() -> anyhow::Result<Self> {
        let config =
            Config::from_env().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;

        let strava_client = Arc::new(StravaClient::new(
            config.strava_client_id,
            config.strava_client_secret,
            config.strava_refresh_token,
        )?);

        let scheduler = JobScheduler::new().await?;

        Ok(Self {
            strava_client,
            scheduler,
        })
    }

    async fn setup_jobs(&self) -> anyhow::Result<()> {
        let client = self.strava_client.clone();

        self.scheduler
            .add(Job::new_async("0 */15 * * * *", move |_uuid, _l| {
                let client = client.clone();
                Box::pin(async move {
                    match client.get_all_activities(1, 50).await {
                        Ok(_) => match client.hide_duplicate_indoor_rides().await {
                            Ok(result) => {
                                if !result.hidden.is_empty() {
                                    println!(
                                        "Hidden {} duplicate indoor bike activities",
                                        result.hidden.len()
                                    );
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to hide duplicates: {}", e);
                            }
                        },
                        Err(e) => {
                            eprintln!("Failed to fetch activities: {}", e);
                        }
                    }
                })
            })?)
            .await?;

        println!("Scheduled jobs configured");
        Ok(())
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.setup_jobs().await?;
        self.scheduler.start().await?;
        println!("Scheduler started. Press Ctrl+C to exit.");
        println!("Sync runs every 15 minutes at :00, :15, :30, :45");

        tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;

        Ok(())
    }
}
