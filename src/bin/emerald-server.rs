use clap::Parser;
use emerald::Vault;
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    folder: PathBuf,
    #[arg(long, default_value = "127.0.0.1:7878")]
    listen: SocketAddr,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    let token = std::env::var("EMERALD_TOKEN")?;
    anyhow::ensure!(
        token.len() >= 32,
        "Set EMERALD_TOKEN to a random secret of at least 32 characters"
    );
    let app = emerald::api().with_state(Vault::new(a.folder, token)?);
    axum::serve(tokio::net::TcpListener::bind(a.listen).await?, app).await?;
    Ok(())
}
