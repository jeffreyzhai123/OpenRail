use std::env;
use std::net::Ipv4Addr;

use sim_api::{Config, app};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::var("PORT").ok();
    let allowed_origin = env::var("ALLOWED_ORIGIN").ok();
    let config = Config::parse(port.as_deref(), allowed_origin.as_deref())?;

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.port)).await?;
    eprintln!("sim-api listening on {}", listener.local_addr()?);
    axum::serve(listener, app(config.allowed_origin))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolves on Ctrl-C or SIGTERM, which Fly sends before stopping a machine,
/// so in-flight requests finish before the server exits. If a handler can't
/// be installed, that signal is ignored rather than shutting down at once.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
