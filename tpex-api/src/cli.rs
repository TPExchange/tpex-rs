use std::pin::pin;

use clap::Parser;
use futures::StreamExt;
use tpex_api::Token;

#[derive(clap::Subcommand)]
enum Command {
    /// Stream the state to stdout
    Mirror,
    /// Create an atomically updated cache file of the FastSync data
    FastsyncCache {
        path: String
    }
}

#[derive(clap::Parser)]
struct Args {
    /// The remote TPEx api endpoint
    #[arg(long, env = "TPEX_URL")]
    endpoint: reqwest::Url,
    /// The token for that remote
    #[arg(long, env = "TPEX_TOKEN")]
    token: Token,
    #[command(subcommand)]
    command: Command
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    match args.command {
        Command::Mirror => {
            let mut next_id = 1;
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let state_stream = remote.stream_state(next_id).await.expect("Failed to stream state");
            let mut state_stream = pin!(state_stream);
            while let Some(next) = state_stream.next().await {
                let next = next.unwrap();
                assert_eq!(next.id, next_id, "Skipped id in state");
                serde_json::to_writer(&std::io::stdout(), &next).expect("Failed to reserialise wrapped action");
                println!();
                next_id += 1;
            }
        },
        Command::FastsyncCache { path } => {
            let tmp_path = format!("{path}.tmp");
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let state_stream = remote.stream_fastsync().await.expect("Failed to stream fastsync");
            let mut state_stream = pin!(state_stream);
            while let Some(next) = state_stream.next().await {
                let next = next.unwrap();
                println!("Id: {}", next.current_id);
                // Atomic overwrite of file
                tokio::fs::write(&tmp_path, serde_json::to_string(&next).unwrap()).await.expect("Could not write cached data");
                std::fs::rename(&tmp_path, &path).expect("Failed to overwrite cached data");
            }
        }
    }

}
