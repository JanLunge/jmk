use std::net::SocketAddr;
use std::process::ExitCode;

use jmk_registry::{app, AppState, Config};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("jmk-registry: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env()?;
    let bind = config.bind;
    let state = AppState::open(config)?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| format!("cannot listen on {bind}: {error}"))?;
    eprintln!("jmk-registry: listening on {bind}");
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(|error| format!("server stopped: {error}"))
}

/// Resolves on Ctrl-C or SIGTERM so a container stop closes the database cleanly.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => tokio::select! {
                _ = interrupt => {}
                _ = terminate.recv() => {}
            },
            Err(_) => interrupt.await,
        }
    }
    #[cfg(not(unix))]
    interrupt.await;
}
