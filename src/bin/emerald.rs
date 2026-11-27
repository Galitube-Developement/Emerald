#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]
use clap::Parser;
use emerald::Vault;
use std::path::PathBuf;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    vault: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    let root = a.vault.unwrap_or_else(|| {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default()
            .join("Emerald")
    });
    let token = hex::encode(rand::random::<[u8; 32]>());
    let rt = tokio::runtime::Runtime::new()?;
    let listener = rt.block_on(tokio::net::TcpListener::bind(("127.0.0.1", 0)))?;
    let url = format!("http://{}/#{}", listener.local_addr()?, token);
    let app = emerald::client().with_state(Vault::new(root, token)?);
    rt.spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    #[cfg(target_os = "linux")]
    gtk_init();

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Emerald Vault")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .build(&event_loop)?;
    let builder = WebViewBuilder::new().with_url(&url);
    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().unwrap())?
    };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        let _keep = (&rt, &_webview);
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *flow = ControlFlow::Exit;
        }
    });
}

#[cfg(target_os = "linux")]
fn gtk_init() {}
