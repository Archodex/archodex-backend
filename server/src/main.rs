use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[cfg(debug_assertions)]
const RUNTIME_STACK_SIZE: usize = 20 * 1024 * 1024; // 20MiB in debug mode
#[cfg(not(debug_assertions))]
const RUNTIME_STACK_SIZE: usize = 10 * 1024 * 1024; // 10MiB in release mode

const PORT: u16 = 5731;

fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,surreal=debug".into()))
        .init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(RUNTIME_STACK_SIZE)
        .build()
        .unwrap()
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{PORT}"))
                .await
                .expect(&format!("Failed to listen on port {PORT}"));

            info!("Listening on port {PORT}");

            axum::serve(listener, archodex_backend::router::router()).await
        })
}
