use std::fmt::Debug;

use crate::{shared::*, state_type};
pub mod tokens;
pub mod state;

use axum::{extract::{ws::rejection::WebSocketUpgradeRejection, FromRequestParts}, response::IntoResponse, serve::Listener, Router};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite};
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tpex::{ActionLevel, AssetId, AssetInfo, StateSync};
#[derive(clap::Parser)]
pub struct Args {
    pub trades: std::path::PathBuf,
    pub db: String,
    pub endpoint: String,
    pub assets: Option<std::path::PathBuf>,
}

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
    axum::extract::State(state): axum::extract::State<state_type!()>,
    token: TokenInfo,
    axum_extra::extract::OptionalQuery(args): axum_extra::extract::OptionalQuery<StatePatchArgs>,
    axum::extract::Json(action): axum::extract::Json<tpex::Action>
) -> Result<axum::response::Json<u64>, Error> {
    match token.level {
        TokenLevel::ReadOnly => return Err(Error::TokenTooLowLevel),
        TokenLevel::ProxyOne => {
            let perms = state.tpex.read().await.state().perms(&action)?;
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
            let next_id = tpex_state.state().get_next_id();
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

struct OptionalWebSocket(pub Option<axum::extract::ws::WebSocketUpgrade>);
impl<S : Send + Sync> FromRequestParts<S> for OptionalWebSocket {
    #[doc = " If the extractor fails it\'ll use this \"rejection\" type. A rejection is"]
    #[doc = " a kind of error that can be converted into a response."]
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request_parts(parts: &mut axum::http::request::Parts,state: &S,) -> Result<Self,Self::Rejection> {
        match axum::extract::ws::WebSocketUpgrade::from_request_parts(parts, state).await {
            Ok(x) => Ok(Self(Some(x))),
            Err(WebSocketUpgradeRejection::MethodNotGet(_)) |
            Err(WebSocketUpgradeRejection::MethodNotConnect(_)) |
            Err(WebSocketUpgradeRejection::InvalidConnectionHeader(_)) |
            Err(WebSocketUpgradeRejection::InvalidUpgradeHeader(_)) => Ok(Self(None)),
            Err(e) => Err(e)
        }
    }
}

async fn state_get(
    axum::extract::State(state): axum::extract::State<state_type!()>,
    // must extract token to auth
    _token: TokenInfo,
    axum_extra::extract::OptionalQuery(args): axum_extra::extract::OptionalQuery<StateGetArgs>,
    OptionalWebSocket(upgrade): OptionalWebSocket
) -> axum::response::Response {
    let mut from = args.unwrap_or_default().from.unwrap_or(0);
    if let Some(upgrade) = upgrade {
        upgrade.on_upgrade(move |mut sock: axum::extract::ws::WebSocket| async move {
            let mut subscription = state.updated.subscribe();
            loop {
                subscription.wait_for(|i| *i >= from).await.expect("Failed to poll updated_recv");

                let tpex_state_handle = state.tpex.read().await;
                // It's better to clone these out than hold state
                let res =
                    tpex_state_handle.cache().iter()
                    .skip(from as usize)
                    .map(Into::into)
                    .map(axum::extract::ws::Message::Text)
                    .collect::<Vec<_>>();
                // rechecking the id prevents a race condition
                from = tpex_state_handle.state().get_next_id() - 1;
                // We have extracted all we need
                drop(tpex_state_handle);
                // Send it off
                for i in res {
                    if sock.send(i).await.is_err() {
                        break;
                    }
                }
            }
        })
    }
    else {
        let data =
            state.tpex.read().await.cache().iter()
            .skip(from as usize)
            .fold(String::new(), |a, b| a + b);
        let body = axum::body::Body::from(data);
        axum::response::Response::builder()
        .header("Content-Type", "text/plain")
        .body(body)
        .expect("Unable to create state_get response")
    }
}

async fn token_get(
    axum::extract::State(_state): axum::extract::State<state_type!()>,
    token: TokenInfo
) -> axum::Json<TokenInfo> {
    axum::Json(token)
}

async fn token_post(
    axum::extract::State(state): axum::extract::State<state_type!()>,
    token: TokenInfo,
    axum::extract::Json(args): axum::extract::Json<TokenPostArgs>,
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
    axum::extract::State(state): axum::extract::State<state_type!()>,
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
    axum::extract::State(state): axum::extract::State<state_type!()>,
    _token: TokenInfo,
    OptionalWebSocket(upgrade): OptionalWebSocket
) -> axum::response::Response {
    if let Some(upgrade) = upgrade {
        upgrade.on_upgrade(move |mut sock: axum::extract::ws::WebSocket| async move {
            let mut subscription = state.updated.subscribe();
            subscription.mark_changed();
            loop {
                subscription.changed().await.expect("Failed to poll updated_recv");
                let res = StateSync::from(state.tpex.read().await.state());
                if sock.send(axum::extract::ws::Message::Text(serde_json::to_string(&res).expect("Could not serialise state sync").into())).await.is_err() {
                    break;
                }
            }
        })
    }
    else {
        let res = StateSync::from(state.tpex.read().await.state());
        axum::Json(res).into_response()
    }
}

pub async fn run_server<L: Listener>(
    cancel: CancellationToken,
    mut trade_log: impl AsyncWrite + AsyncBufRead + AsyncSeek + Unpin + Send + Sync + 'static,
    token_handler: tokens::TokenHandler,
    listener: L,
    assets: Option<std::collections::HashMap<AssetId, AssetInfo>>)
    where L::Addr : Debug
{
    // Load cache
    let mut cache = Vec::new();
    {
        let mut lines = trade_log.lines();
        while let Some(mut line) = lines.next_line().await.expect("Could not read trade file") {
            line.push('\n');
            cache.push(line);
        }
        trade_log = lines.into_inner();
        trade_log.rewind().await.expect("Could not rewind trade file");
    }

    let mut tpex_state = tpex::State::new();
    if let Some(assets) = assets {
        tpex_state.update_asset_info(assets)
    }
    tpex_state.replay(&mut trade_log, true).await.expect("Could not replay trades");

    let (updated, _) = tokio::sync::watch::channel(tpex_state.get_next_id().checked_sub(1).expect("Poll counter underflow"));
    let state = state::StateStruct {
        tpex: tokio::sync::RwLock::new(state::TPExState::new(tpex_state, trade_log, cache)),
        tokens: token_handler,
        updated
    };

    let cors = tower_http::cors::CorsLayer::new()
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any);


    let app = Router::new()
        .route("/state", axum::routing::get(state_get))
        .route("/state", axum::routing::connect(state_get))
        .route("/state", axum::routing::patch(state_patch))

        .route("/token", axum::routing::get(token_get))
        .route("/token", axum::routing::post(token_post))
        .route("/token", axum::routing::delete(token_delete))

        .route("/fastsync", axum::routing::get(fastsync_get))

        .with_state(std::sync::Arc::new(state))

        .layer(TraceLayer::new_for_http())

        .route_layer(cors);

    axum::serve(listener, app).with_graceful_shutdown(async move { cancel.cancelled().await }).await.expect("Failed to serve");
}

pub async fn run_server_with_args(args: Args, cancel: CancellationToken) {
    run_server(
        cancel,
        tokio::io::BufStream::with_capacity(16<<20, 16<<20,
            tokio::fs::File::options()
            .read(true)
            .write(true)
            .truncate(false)
            .create(true)
            .open(args.trades).await.expect("Unable to open trade list")),
        tokens::TokenHandler::new(&args.db).await.expect("Could not connect to DB"),
        tokio::net::TcpListener::bind(args.endpoint).await.expect("Could not bind to endpoint"),
        match args.assets {
            Some(asset_path) => {
                let mut assets = String::new();
                tokio::fs::File::open(asset_path).await.expect("Unable to open asset info")
                .read_to_string(&mut assets).await.expect("Unable to read asset list");

                Some(serde_json::from_str(&assets).expect("Unable to parse asset info"))
            },
            None=> None
        }
    ).await
}
