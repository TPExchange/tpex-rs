mod tests;

mod tokens;
mod shared;

use shared::*;

use axum::Router;
use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tower_http::trace::TraceLayer;
use tpex::{Action, ActionLevel, StateSync};
use std::io::Write;
use tracing_subscriber::EnvFilter;

#[derive(clap::Parser)]
struct Args {
    trades: std::path::PathBuf,
    db: String,
    endpoint: String,
    assets: Option<std::path::PathBuf>,
}

struct TPExState {
    state: tpex::State,
    file: tokio::fs::File
}
impl TPExState {
    async fn apply(&mut self, action: Action) -> Result<u64, tpex::Error> {
        self.state.apply(action, &mut self.file).await
    }
    async fn get_lines(&mut self) -> Vec<u8> {
        // Keeping everything in the log file means we can't have different versions of the same data
        self.file.rewind().await.expect("Could not rewind trade file.");
        let mut buf = Vec::new();
        // This will seek to the end again, so pos is the same before and after get_lines
        self.file.read_to_end(&mut buf).await.expect("Could not re-read trade file.");
        buf
    }
}

struct StateStruct {
    tpex: tokio::sync::RwLock<TPExState>,
    tokens: tokens::TokenHandler,
    updated: tokio::sync::watch::Sender<u64>,
}
type State = std::sync::Arc<StateStruct>;

#[derive(Debug)]
enum Error {
    TPEx(tpex::Error),
    UncontrolledUser,
    TokenTooLowLevel,
    TokenInvalid,
    NotNextId{next_id: u64}
}
impl From<tpex::Error> for Error {
    fn from(value: tpex::Error) -> Self {
        Self::TPEx(value)
    }
}
impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let (code,err) = match self {
            Self::TPEx(err) => (409, ErrorInfo{error:err.to_string()}),
            Self::UncontrolledUser => (403, ErrorInfo{error:"This action would act on behalf of a different user.".to_owned()}),
            Self::TokenTooLowLevel => (403, ErrorInfo{error:"This action requires a higher permission level".to_owned()}),
            Self::NotNextId{next_id} => (409, ErrorInfo{error:format!("The requested ID was not the next, which is {next_id}")}),
            Self::TokenInvalid => (409, ErrorInfo{error:"The given token does not exist".to_owned()})
        };

        let body = serde_json::to_vec(&err).expect("Unable to serialise error");

        let body = axum::body::Body::from(body);

        axum::response::Response::builder()
        .status(code)
        .header("Content-Type", "application/json")
        .body(body)
        .expect("Unable to create error response")
    }
}

async fn state_patch(
    axum::extract::State(state): axum::extract::State<State>,
    token: TokenInfo,
    axum_extra::extract::OptionalQuery(args): axum_extra::extract::OptionalQuery<StatePatchArgs>,
    axum::extract::Json(action): axum::extract::Json<tpex::Action>
) -> Result<axum::response::Json<u64>, Error> {
    match token.level {
        TokenLevel::ReadOnly => return Err(Error::TokenTooLowLevel),
        TokenLevel::ProxyOne => {
            let perms = state.tpex.read().await.state.perms(&action)?;
            if perms.player != token.user {
                return Err(Error::UncontrolledUser);
            }
            if perms.level > ActionLevel::Normal {
                return Err(Error::TokenTooLowLevel);
            }
        }
        // Apply catches all banker perm mismatches, assuming that upstream has verified their action:
        TokenLevel::ProxyAll => ()
    }
    let mut tpex_state = state.tpex.write().await;
    let id =
        if let Some(expected_id) = args.and_then(|i| i.id) {
            let next_id = tpex_state.state.get_next_id();
            if next_id != expected_id {
                return Err(Error::NotNextId{next_id});
            }
            let id = tpex_state.apply(action).await?;
            assert_eq!(id, next_id, "Somehow got ID mismatch");
            id
        }
        else {
            tpex_state.apply(action).await?
        };
    // We patched, so update the id
    //
    // We use send_replace so that we don't have to worry about if anyone's listening
    state.updated.send_replace(id);
    Ok(axum::Json(id))
}

async fn state_get(
    axum::extract::State(state): axum::extract::State<State>,
    // must extract token to auth
    _token: TokenInfo,
    axum_extra::extract::OptionalQuery(args): axum_extra::extract::OptionalQuery<StateGetArgs>
) -> axum::response::Response {
    let from = args.unwrap_or_default().from.unwrap_or(0).try_into().unwrap_or(usize::MAX);
    let mut data = state.tpex.write().await.get_lines().await;
    if from > 1 {
        let idx =
            data.iter()
            .enumerate()
            .filter(|(_, i)| **i == b'\n')
            .map(|(idx,_)| idx)
            .nth(from - 2)
            .unwrap_or(usize::MAX);
        if idx >= data.len() {
            data = Vec::new();
        }
        else {
            data.drain(0..=idx);
        }
    }
    let body = axum::body::Body::from(data);
    axum::response::Response::builder()
    .header("Content-Type", "text/plain")
    .body(body)
    .expect("Unable to create state_get response")
}

async fn token_get(
    axum::extract::State(_state): axum::extract::State<State>,
    token: TokenInfo
) -> axum::Json<TokenInfo> {
    axum::Json(token)
}

async fn token_post(
    axum::extract::State(state): axum::extract::State<State>,
    token: TokenInfo,
    axum::extract::Json(args): axum::extract::Json<TokenPostArgs>
) -> Result<axum::Json<Token>, Error> {
    if args.level > token.level {
        return Err(Error::TokenTooLowLevel)
    }
    if args.user != token.user && token.level < TokenLevel::ProxyAll {
        return Err(Error::UncontrolledUser)
    }

    Ok(axum::Json(state.tokens.create_token(args.level, args.user).await.expect("Cannot access DB")))
}

async fn token_delete(
    axum::extract::State(state): axum::extract::State<State>,
    token: TokenInfo,
    axum::extract::Json(args): axum::extract::Json<TokenDeleteArgs>
) -> Result<axum::Json<()>, Error> {
    let target = args.token.unwrap_or(token.token);
    // We only need perms to delete other tokens
    if target != token.token && token.level < TokenLevel::ProxyAll {
        return Err(Error::TokenTooLowLevel);
    }
    state.tokens.delete_token(&token.token).await
    .map_or(Err(Error::TokenInvalid), |_| Ok(axum::Json(())))
}

async fn fastsync_get(
    axum::extract::State(state): axum::extract::State<State>,
    _token: TokenInfo
) -> axum::Json<StateSync> {
    let res = StateSync::from(&state.tpex.read().await.state);
    axum::Json(res)
}

async fn poll_get(
    axum::extract::State(state): axum::extract::State<State>,
    _token: TokenInfo,
    upgrade: axum::extract::ws::WebSocketUpgrade
) -> axum::response::Response {
    upgrade.on_upgrade(move |mut sock: axum::extract::ws::WebSocket| async move {
        while let Some(Ok(id)) = sock.recv().await {
            let axum::extract::ws::Message::Text(data) = id
            else { break; };

            let Some(id): Option<u64> = serde_json::from_slice(data.as_bytes()).ok()
            else { break; };

            let new_id = *state.updated.subscribe().wait_for(|i| *i >= id).await.expect("Failed to poll updated_recv");

            if sock.send(axum::extract::ws::Message::Text(serde_json::to_string(&new_id).expect("Could not serialise new id").into())).await.is_err() {
                break;
            }
        }
        let _ = sock.send(axum::extract::ws::Message::Close(None)).await;
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new("info"))
                .unwrap(),
        )
        .init();


    sqlx::any::install_default_drivers();
    // Crash on inconsistency
    std::panic::set_hook(Box::new(move |info| {
        let _ = writeln!(std::io::stderr(), "{}", info);
        std::process::exit(1);
    }));


    let args = Args::parse();

    let mut trade_file = tokio::fs::File::options().read(true).write(true).truncate(false).create(true).open(args.trades).await.expect("Unable to open trade list");
    let mut tpex_state = tpex::State::new();
    if let Some(asset_path) = args.assets {
        let mut assets = String::new();
        tokio::fs::File::open(asset_path).await.expect("Unable to open asset info")
        .read_to_string(&mut assets).await.expect("Unable to read asset list");

        tpex_state.update_asset_info(serde_json::from_str(&assets).expect("Unable to parse asset info"))
    }
    tpex_state.replay(&mut trade_file, true).await.expect("Could not replay trades");

    let token_handler = tokens::TokenHandler::new(&args.db).await.expect("Could not connect to DB");

    let (updated, _) = tokio::sync::watch::channel(tpex_state.get_next_id().checked_sub(1).expect("Poll counter underflow"));
    let state = StateStruct {
        tpex: tokio::sync::RwLock::new(TPExState { state: tpex_state, file: trade_file }),
        tokens: token_handler,
        updated
    };

    let cors = tower_http::cors::CorsLayer::new()
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any);


    let app = Router::new()
        .route("/state", axum::routing::get(state_get))
        .route("/state", axum::routing::patch(state_patch))

        .route("/token", axum::routing::get(token_get))
        .route("/token", axum::routing::post(token_post))
        .route("/token", axum::routing::delete(token_delete))

        .route("/fastsync", axum::routing::get(fastsync_get))

        .route("/poll", axum::routing::get(poll_get))

        .with_state(std::sync::Arc::new(state))

        .layer(TraceLayer::new_for_http())

        .route_layer(cors);

    let listener = tokio::net::TcpListener::bind(args.endpoint).await.expect("Could not bind to endpoint");
    axum::serve(listener, app).await.unwrap();
}
