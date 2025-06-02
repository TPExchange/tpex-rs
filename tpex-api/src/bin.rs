use std::io::Write;

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod shared;
mod server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new("info"))
                .unwrap(),
        )
        .init();

    // Crash on inconsistency
    std::panic::set_hook(Box::new(move |info| {
        let _ = writeln!(std::io::stderr(), "{}", info);
        std::process::exit(1);
    }));

    let cancel = tokio_util::sync::CancellationToken::new();
    server::run_server_with_args(server::Args::parse(), cancel).await;
}
