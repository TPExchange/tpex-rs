use axum::Router;
use clap::Parser;
use tokio::io::AsyncReadExt;


#[derive(clap::Parser)]
struct Args {
    trades: std::path::PathBuf,
    assets: std::path::PathBuf,
    endpoint: String,
    db: String
}

struct StateStruct {
    tpex: tokio::sync::RwLock<tpex::State>,
}
type State = std::sync::Arc<StateStruct>;

async fn action_post(axum::extract::State(state): axum::extract::State<State>, axum::extract::Json(action): axum::extract::Json<tpex::Action>) {
    // Check if banker
    // Then do stuff
}

async fn subscribe_update() {
    // Return if something happened, maybe websocket?
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut assets = String::new();
    tokio::fs::File::open(args.assets).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list");

    let mut trade_file = tokio::fs::File::options().read(true).write(true).truncate(false).create(true).open(args.trades).await.expect("Unable to open trade list");
    let state = tpex::State::replay(&mut trade_file, serde_json::from_str(&assets).expect("Unable to parse asset info")).await.expect("Could not replay trades");

    let app = Router::new()
        .route("/action", axum::routing::post(action_post(state)))
        .with_state(tokio::sync::RwLock::new(state));

    // hand out three types: data view, impersonate, and banker/admin

    let listener = tokio::net::TcpListener::bind(args.endpoint).await.expect("Could not bind to endpoint");
    axum::serve(listener, app).await.unwrap();
}
