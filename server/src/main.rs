use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5731").await.unwrap();

    info!(
        "Listening on port {}",
        listener.local_addr().unwrap().port()
    );

    axum::serve(listener, archodex_backend::router::router())
        .await
        .unwrap();
}
