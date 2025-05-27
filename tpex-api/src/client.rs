#![cfg(feature="client")]

mod tests;
mod shared;

pub use shared::*;
use tpex::{AssetId, AssetInfo, State};

pub use shared::Token;

#[derive(Debug)]
pub enum Error {
    RequestFailure(reqwest::Error),
    TPExFailure(ErrorInfo),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::RequestFailure(err) => write!(f, "Request failure: {err}"),
            Error::TPExFailure(err) => write!(f, "TPEx failure: {}", err.error)
        }
    }
}
impl std::error::Error for Error {}
impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self { Error::RequestFailure(value) }
}
impl From<ErrorInfo> for Error {
    fn from(value: ErrorInfo) -> Self { Error::TPExFailure(value) }
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
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token)).expect("Unable to make token header"));
        Remote {
            client: reqwest::Client::builder().default_headers(headers).build().expect("Unable to build reqwest client"),
            endpoint
        }
    }
    async fn check_response(response: reqwest::Response) -> Result<reqwest::Response> {
        if response.status().is_success() { Ok(response) }
        else { Err(Error::TPExFailure(response.json().await.expect("Invalid error json"))) }
    }

    pub async fn get_state(&self, from: u64) -> Result<Vec<u8>> {
        let mut target = self.endpoint.clone();
        target.query_pairs_mut().append_pair("from", &from.to_string());
        target.path_segments_mut().expect("Unable to nav to /state").push("state");

        Ok(Self::check_response(self.client.get(target).send().await?).await?.bytes().await?.to_vec())
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
    pub async fn sync(&self) -> tokio::sync::RwLockReadGuard<State> {
        let mut state = self.state.write().await;
        let cursor = std::io::Cursor::new(self.remote.get_state(state.get_next_id()).await.expect("Could not fetch state"));
        let mut buf = tokio::io::BufReader::new(cursor);
        state.replay(&mut buf, true).await.expect("State unable to replay");
        state.downgrade()
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
}
