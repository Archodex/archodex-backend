use tracing::info;

#[cfg(debug_assertions)]
const RUNTIME_STACK_SIZE: usize = 20 * 1024 * 1024; // 20MiB in debug mode
#[cfg(not(debug_assertions))]
const RUNTIME_STACK_SIZE: usize = 10 * 1024 * 1024; // 10MiB in release mode

const PORT: u16 = 5731;

fn setup_logging() {
    use std::io::IsTerminal;
    use tracing_subscriber::{
        filter::{EnvFilter, LevelFilter},
        fmt,
    };

    let color = std::io::stdout().is_terminal()
        && (match std::env::var("COLORTERM") {
            Ok(value) => value == "truecolor" || value == "24bit",
            _ => false,
        } || match std::env::var("TERM") {
            Ok(value) => value == "direct" || value == "truecolor",
            _ => false,
        });

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let fmt = fmt().with_env_filter(env_filter);

    if color {
        fmt.event_format(fmt::format().pretty())
            .with_ansi(color)
            .init();
    } else {
        fmt.with_ansi(false).init();
    };
}

fn main() -> Result<(), std::io::Error> {
    setup_logging();

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
