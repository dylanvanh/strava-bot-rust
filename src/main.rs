//! Strava Bot
//!
//! This application runs scheduled cron jobs to automatically manage Strava activity data,
//! including syncing activities and hiding unwanted duplicates to keep your profile clean.

use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};

mod clients;

/// Main entry point for the Strava bot.
///
/// Sets up scheduled jobs to run at specified intervals:
/// - Every 15 minutes: Main Strava sync job
#[tokio::main]
async fn main() -> Result<(), JobSchedulerError> {
    let sched = JobScheduler::new().await?;

    sched
        .add(Job::new("0 */15 * * * *", |_uuid, _l| {
            println!("I run every 15 minutes");
        })?)
        .await?;

    sched.start().await?;

    println!("Scheduler started");

    tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;

    Ok(())
}
