#![cfg(feature="client")]

mod tests;
mod shared;

#[cfg(feature="server")]
pub mod server;

use futures::{StreamExt, TryStreamExt};
use reqwest::StatusCode;
use reqwest_websocket::{Message, RequestBuilderExt};
pub use shared::*;
use tpex::{AssetId, AssetInfo, State, StateSync};

pub use shared::Token;

#[derive(Debug)]
pub enum Error {
    RequestFailure(reqwest::Error),
    WebSocketFailure(reqwest_websocket::Error),
    TPExFailure(ErrorInfo),
    Unknown(Option<StatusCode>)
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::RequestFailure(err) => write!(f, "Request failure: {err}"),
            Error::WebSocketFailure(err) => write!(f, "WebSocket failure: {err}"),
            Error::TPExFailure(err) => write!(f, "TPEx failure: {}", err.error),
            Error::Unknown(Some(code)) => write!(f, "Unknown failure with status code {code}"),
            Error::Unknown(None) => write!(f, "Unknown failutre"),
        }
    }
}
impl std::error::Error for Error {}
impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self { Error::RequestFailure(value) }
}
impl From<reqwest_websocket::Error> for Error {
    fn from(value: reqwest_websocket::Error) -> Self { Error::WebSocketFailure(value) }
}
impl From<ErrorInfo> for Error {
    fn from(value: ErrorInfo) -> Self { Error::TPExFailure(value) }
}
impl From<tpex::Error> for Error {
    fn from(value: tpex::Error) -> Self { Error::TPExFailure(ErrorInfo { error: value.to_string() }) }
}


pub type Result<T> = core::result::Result<T, Error>;

pub struct Remote {
    client: reqwest::Client,
    endpoint: reqwest::Url
}
impl Remote {
    pub fn new(endpoint: reqwest::Url, token: Token) -> Remote {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            "Authorization",
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).expect("Unable to make token header"));
        Remote {
            client: reqwest::Client::builder().default_headers(headers).build().expect("Unable to build reqwest client"),
            endpoint
        }
    }
    async fn check_response(response: reqwest::Response) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() { Ok(response) }
        else if let Ok(err) = response.json().await {
            Err(Error::TPExFailure(err))
        }
        else {
            Err(Error::Unknown(Some(status)))
        }
    }

    pub async fn get_state(&self, from: u64) -> Result<Vec<u8>> {
        let mut target = self.endpoint.clone();
        target.query_pairs_mut().append_pair("from", &from.to_string());
        target.path_segments_mut().expect("Unable to nav to /state").push("state");

        Ok(Self::check_response(self.client.get(target).send().await?).await?.bytes().await?.to_vec())
    }
    pub async fn stream_state(&self, from: u64) -> Result<impl futures::Stream<Item=Result<tpex::WrappedAction>> + use<>> {
        let mut target = self.endpoint.clone();
        target.query_pairs_mut().append_pair("from", &from.to_string());
        target.path_segments_mut().expect("Unable to nav to /state").push("state");

        let ws = self.client.get(target)
            .upgrade()
            .send().await?
            .into_websocket().await?;

        Ok(ws.filter_map(|msg| async {
            let ret: Option<Result<tpex::WrappedAction>> = match msg {
                Ok(Message::Text(text)) => Some(serde_json::from_str(&text).map_err(|_| Error::Unknown(None))),
                Ok(Message::Binary(binary)) => Some(serde_json::from_slice(&binary).map_err(|_| Error::Unknown(None))),
                Err(e) => Some(Err(e.into())),
                _ => None
            };
            ret
        }))
    }
    pub async fn apply(&self, action: &tpex::Action) -> Result<u64> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /state").push("state");

        Ok(Self::check_response(self.client.patch(target).json(action).send().await?).await?.json().await?)
    }
    pub async fn get_token(&self, token: &Token) -> Result<TokenInfo> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /token").push("token");

        Ok(Self::check_response(self.client.post(target).json(token).send().await?).await?.json().await?)
    }
    pub async fn create_token(&self, args: &TokenPostArgs) -> Result<Token> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /token").push("token");

        Ok(Self::check_response(self.client.post(target).json(args).send().await?).await?.json().await?)
    }
    pub async fn delete_token(&self, args: &TokenDeleteArgs) -> Result<()> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /token").push("token");

        Ok(Self::check_response(self.client.delete(target).json(args).send().await?).await?.json().await?)
    }
    pub async fn fastsync(&self) -> Result<StateSync> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /fastsync").push("fastsync");

        Ok(Self::check_response(self.client.get(target).send().await?).await?.json().await?)
    }
    pub async fn stream_fastsync(&self) -> Result<impl futures::Stream<Item=Result<tpex::StateSync>>> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /fastsync").push("fastsync");

        let ws = self.client.get(target)
            .upgrade()
            .send().await?
            .into_websocket().await?;

        Ok(ws.filter_map(|msg| async {
            let ret: Option<Result<tpex::StateSync>> = match msg {
                Ok(Message::Text(text)) => Some(serde_json::from_str(&text).map_err(|_| Error::Unknown(None))),
                Ok(Message::Binary(binary)) => Some(serde_json::from_slice(&binary).map_err(|_| Error::Unknown(None))),
                Err(e) => Some(Err(e.into())),
                _ => None
            };
            ret
        }))
    }
    pub async fn get_balance(&self, player: &tpex::PlayerId) -> Result<tpex::Coins> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /inspect/balance").push("inspect").push("balance");
        target.query_pairs_mut().append_pair("player", player.get_raw_name());

        Ok(Self::check_response(self.client.get(target).send().await?).await?.json().await?)
    }
    pub async fn get_assets(&self, player: &tpex::PlayerId) -> Result<std::collections::HashMap<AssetId, u64>> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /inspect/assets").push("inspect").push("assets");
        target.query_pairs_mut().append_pair("player", player.get_raw_name());

        Ok(Self::check_response(self.client.get(target).send().await?).await?.json().await?)
    }
    pub async fn itemised_audit(&self) -> Result<tpex::ItemisedAudit> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /inspect/audit").push("inspect").push("audit");

        Ok(Self::check_response(self.client.get(target).send().await?).await?.json().await?)
    }
}

pub struct Mirrored {
    pub remote: Remote,
    state: tokio::sync::RwLock<State>
}
impl Mirrored {
    pub fn new(endpoint: reqwest::Url, token: Token) -> Mirrored {
        Mirrored {
            remote: Remote::new(endpoint, token),
            state: tokio::sync::RwLock::new(State::new())
        }
    }
    pub async fn update_asset_info(&self, asset_info: std::collections::HashMap<AssetId, AssetInfo>) {
        self.state.write().await.update_asset_info(asset_info)
    }
    pub async fn fastsync(&'_ self) -> Result<tokio::sync::RwLockReadGuard<'_, State>> {
        let new_state: State = self.remote.fastsync().await?.try_into()?;
        let mut state = self.state.write().await;
        *state = new_state;
        Ok(state.downgrade())
    }
    pub async fn sync(&'_ self) -> Result<tokio::sync::RwLockReadGuard<'_, State>> {
        let mut state = self.state.write().await;
        let cursor = std::io::Cursor::new(self.remote.get_state(state.get_next_id() - 1).await?);
        let mut buf = tokio::io::BufReader::new(cursor);
        state.replay(&mut buf, true).await.expect("State unable to replay");
        Ok(state.downgrade())
    }
    pub async fn apply(&self, action: tpex::Action) -> Result<u64> {
        // The remote could be desynced, so we send our update
        let id = self.remote.apply(&action).await?;
        drop(self.sync().await);
        Ok(id)
    }
    // This isn't synced
    pub async fn asset_info(&self, asset: &AssetId) -> std::result::Result<AssetInfo, tpex::Error> {
        self.state.read().await.asset_info(asset)
    }
    pub async fn stream(self: std::sync::Arc<Self>) -> Result<impl futures::Stream<Item=Result<(std::sync::Arc<Self>, tpex::WrappedAction)>>> {
        let next_id = self.state.read().await.get_next_id();
        let this: std::sync::Arc<Self> = self.clone();
        let stream = self.remote.stream_state(next_id).await?;
        Ok(stream.and_then(move |wrapped_action| { let this = this.clone(); async move  {
            this.state.write().await.apply(wrapped_action.action.clone(), tokio::io::sink()).await?;
            Ok((this, wrapped_action))
        }}))
    }
}
