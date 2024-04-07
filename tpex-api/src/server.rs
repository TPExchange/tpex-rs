mod tokens;
mod shared;

use shared::*;

use axum::Router;
use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use serde::ser::{Serializer, SerializeMap};
use tpex::{Action, ActionLevel};

#[derive(clap::Parser)]
struct Args {
    trades: std::path::PathBuf,
    assets: std::path::PathBuf,
    endpoint: String,
    db: String
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
    tokens: tokens::TokenHandler
}
type State = std::sync::Arc<StateStruct>;

#[derive(Debug)]
enum Error {
    TPEx(tpex::Error),
    UncontrolledUser,
    TokenTooLowLevel,
    TokenInvalid
}
impl From<tpex::Error> for Error {
    fn from(value: tpex::Error) -> Self {
        Self::TPEx(value)
    }
}
impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let (code,msg) = match self {
            Self::TPEx(err) => (409, err.to_string()),
            Self::UncontrolledUser => (403, "This action would act on behalf of a different user.".to_owned()),
            Self::TokenTooLowLevel => (403, "This actions requires a higher permission level".to_owned()),
            Self::TokenInvalid => (409, "The given token does not exist".to_owned())
        };

        let mut body = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut body);
        let mut err_msg = ser.serialize_map(None).unwrap();
        err_msg.serialize_entry("error", &msg).unwrap();
        err_msg.end().unwrap();


        let body = axum::body::Body::from(body);

        axum::response::Response::builder()
        .status(code)
        .header("Content-Type", "application/json")
        .body(body)
        .expect("Unable to create error response")
    }
}

#[axum::debug_handler]
async fn state_patch(
    axum::extract::State(state): axum::extract::State<State>,
    token: tokens::TokenInfo,
    axum::extract::Json(action): axum::extract::Json<tpex::Action>)
    -> Result<axum::response::Json<u64>, Error>
{
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
    let id = state.tpex.write().await.apply(action).await?;
    Ok(axum::Json(id))
}
#[axum::debug_handler]
async fn state_get(
    axum::extract::State(state): axum::extract::State<State>,
    // must extract token to auth
    _token: tokens::TokenInfo,
    axum::extract::Query(from): axum::extract::Query<Option<u64>>)
    -> axum::response::Response
{
    let from = from.unwrap_or(0).try_into().unwrap_or(usize::MAX);
    let mut data = state.tpex.write().await.get_lines().await;
    if from > 0 {
        let idx =
            data.iter()
            .enumerate()
            .filter(|(_, i)| **i == b'\n')
            .map(|(idx,_)| idx)
            .nth(from - 1)
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

#[axum::debug_handler]
async fn token_post(
    axum::extract::State(state): axum::extract::State<State>,
    token: tokens::TokenInfo,
    axum::extract::Json(args): axum::extract::Json<TokenPostArgs>)
    -> Result<axum::Json<Token>, Error>
{
    if args.level < token.level {
        return Err(Error::TokenTooLowLevel)
    }
    if args.user != token.user && token.level < TokenLevel::ProxyAll {
        return Err(Error::UncontrolledUser)
    }

    Ok(axum::Json(state.tokens.create_token(args.level, args.user).await.expect("Cannot access DB")))
}

#[axum::debug_handler]
async fn token_delete(
    axum::extract::State(state): axum::extract::State<State>,
    token: tokens::TokenInfo,
    axum::extract::Json(args): axum::extract::Json<TokenDeleteArgs>)
    -> Result<axum::Json<()>, Error>
{
    let target = args.token.unwrap_or(token.token);
    // We only need perms to delete other tokens
    if target != token.token && token.level < TokenLevel::ProxyAll {
        return Err(Error::TokenTooLowLevel);
    }
    state.tokens.delete_token(&token.token).await
    .map_or(Err(Error::TokenInvalid), |_| Ok(axum::Json(())))
}
#[tokio::main]
async fn main() {
    sqlx::any::install_default_drivers();

    let args = Args::parse();

    let mut assets = String::new();
    tokio::fs::File::open(args.assets).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list");

    let mut trade_file = tokio::fs::File::options().read(true).write(true).truncate(false).create(true).open(args.trades).await.expect("Unable to open trade list");
    let tpex_state = tpex::State::replay(&mut trade_file, serde_json::from_str(&assets).expect("Unable to parse asset info")).await.expect("Could not replay trades");
    let token_handler = tokens::TokenHandler::new(&args.db).await.expect("Could not connect to DB");
    let state = StateStruct {
        tpex: tokio::sync::RwLock::new(TPExState { state: tpex_state, file: trade_file }),
        tokens: token_handler
    };

    let app = Router::new()
        .route("/state", axum::routing::patch(state_patch))
        .route("/state", axum::routing::get(state_get))
        .route("/token", axum::routing::post(token_post))
        .route("/token", axum::routing::delete(token_delete))
        .with_state(std::sync::Arc::new(state));

    let listener = tokio::net::TcpListener::bind(args.endpoint).await.expect("Could not bind to endpoint");
    axum::serve(listener, app).await.unwrap();
}
