#[tokio::main]
async fn main() {
    let argv: Vec<_> = std::env::args().collect();
    if argv.len() != 2 {
        println!("TPEx validator requires a single argument: the path to the trade list");
        return;
    }
    let mut txlog = tokio::io::BufReader::new(tokio::fs::OpenOptions::new().read(true).open(&argv[1]).await.expect("Could not open txlog"));

    let mut state = tpex::State::default();
    if let Err(e) = state.replay(&mut txlog, true).await {
        println!("Failed to replay line {} of state: {e}", state.get_next_id());
        return;
    }

    println!("State replayed successfully:");
    println!("{}",serde_json::to_string_pretty(&tpex::StateSync::from(&state)).expect("Could not serialise state"));
}
