mod app;
mod clients;
mod config;

use app::App;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let app = App::new().await?;
    app.run().await?;

    Ok(())
}
