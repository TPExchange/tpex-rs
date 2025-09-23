use tokio::io::AsyncBufReadExt;

#[tokio::main]
async fn main() {
    let argv: Vec<_> = std::env::args().collect();
    if argv.len() != 2 {
        println!("TPEx validator requires a single argument: the path to the trade list");
        return;
    }
    let txlog = tokio::io::BufReader::new(tokio::fs::OpenOptions::new().read(true).open(&argv[1]).await.expect("Could not open txlog"));

    let mut state = tpex::State::default();
    let mut lines = txlog.lines();
    let mut line_no: u64 = 0;
    while let Some(line) = lines.next_line().await.expect("Could not read file") {
        line_no += 1;
        eprint!("{line_no}: ");
        let wrapped_action: tpex::WrappedAction = serde_json::from_str(&line).expect("Failed to parse wrapped action");
        assert_eq!(wrapped_action.id, line_no);
        state.apply_with_time(wrapped_action.action, wrapped_action.time, tokio::io::sink()).await.expect("Failed to apply action");
        eprintln!("OK");
    }

    println!("State replayed successfully:");
    println!("{}",serde_json::to_string_pretty(&tpex::StateSync::from(&state)).expect("Could not serialise state"));
}
