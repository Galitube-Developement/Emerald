use clap::Parser;
use emerald::Vault;
use std::path::PathBuf;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    vault: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    no_browser: bool,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    let root = a.vault.unwrap_or_else(|| {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default()
            .join("Emerald")
    });
    let token = hex::encode(rand::random::<[u8; 32]>());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", a.port)).await?;
    let url = format!("http://{}/#{}", listener.local_addr()?, token);
    if a.no_browser || webbrowser::open(&url).is_err() {
        println!("Open this private local URL: {url}");
    }
    axum::serve(
        listener,
        emerald::client().with_state(Vault::new(root, token)?),
    )
    .await?;
    Ok(())
}
