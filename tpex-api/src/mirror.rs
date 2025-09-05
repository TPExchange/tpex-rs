use std::{env::args, pin::pin};

use futures::StreamExt;

fn help_message_then_die<T>() -> T {
    eprintln!("Usage: {} <endpoint> <token>", args().next().as_ref().map(AsRef::<str>::as_ref).unwrap_or("tpex-mirror"));
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let ep = args().nth(1).unwrap_or_else(help_message_then_die);
    let token = args().nth(2).unwrap_or_else(help_message_then_die);
    let mut next_id = 1;
    'next: loop {
        let remote = tpex_api::Remote::new(ep.parse().expect("Invalid URL parsed for endpoint"), token.parse().expect("Invalid token given"));
        let Ok(state_stream) = remote.stream_state(next_id).await else {continue;};
        let mut state_stream = pin!(state_stream);
        while let Some(next) = state_stream.next().await {
            let Ok(next) = next else { continue 'next; };
            if next.id != next_id {
                continue 'next;
            }
            serde_json::to_writer(&std::io::stdout(), &next).expect("Failed to reserialise wrapped action");
            println!();
            next_id += 1;
        }
    }
}
