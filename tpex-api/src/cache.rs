use std::{env::args, pin::pin};

use futures::StreamExt;

fn help_message_then_die<T>() -> T {
    eprintln!("Usage: {} <endpoint> <token> <output_path>", args().next().as_ref().map(AsRef::<str>::as_ref).unwrap_or("tpex-mirror"));
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let ep = args().nth(1).unwrap_or_else(help_message_then_die);
    let token = args().nth(2).unwrap_or_else(help_message_then_die);
    let path = args().nth(3).unwrap_or_else(help_message_then_die);
    let tmp_path = format!("{path}.tmp");
    'next: loop {
        let remote = tpex_api::Remote::new(ep.parse().expect("Invalid URL parsed for endpoint"), token.parse().expect("Invalid token given"));
        let Ok(state_stream) = remote.stream_fastsync().await else {continue;};
        let mut state_stream = pin!(state_stream);
        while let Some(next) = state_stream.next().await {
            let Ok(next) = next else { continue 'next; };
            println!("Id: {}", next.current_id);
            // Atomic overwrite of file
            tokio::fs::write(&tmp_path, serde_json::to_string(&next).unwrap()).await.expect("Could not write cached data");
            std::fs::rename(&tmp_path, &path).expect("Failed to overwrite cached data");
        }
    }
}
