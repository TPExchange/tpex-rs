mod shared;
use shared::*;
use tpex::{AssetId, AssetInfo, State};

pub use shared::Token;

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

    pub async fn get_state(&self, from: u64) -> reqwest::Result<Vec<u8>> {
        let mut target = self.endpoint.clone();
        target.query_pairs_mut().append_pair("from", &from.to_string());
        target.path_segments_mut().expect("Unable to nav to /state").push("/state");

        Ok(self.client.get(target).send().await?.bytes().await?.to_vec())
    }
    pub async fn apply(&self, action: &tpex::Action) -> reqwest::Result<u64> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /state").push("/state");

        self.client.patch(target).json(action).send().await?.json().await
    }
    pub async fn create_token(&self, args: &TokenPostArgs) -> reqwest::Result<Token> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /token").push("/token");

        self.client.post(target).json(args).send().await?.json().await
    }
    pub async fn delete_token(&self, args: &TokenPostArgs) -> reqwest::Result<()> {
        let mut target = self.endpoint.clone();
        target.path_segments_mut().expect("Unable to nav to /token").push("/token");

        self.client.delete(target).json(args).send().await?.json().await
    }
}

pub struct Mirrored {
    pub remote: Remote,
    state: tokio::sync::RwLock<State>
}
impl Mirrored {
    pub async fn new(asset_info: std::collections::HashMap<AssetId, AssetInfo>, endpoint: reqwest::Url, token: Token) -> Mirrored {
        let ret = Mirrored {
            remote: Remote::new(endpoint, token),
            state: tokio::sync::RwLock::new(State::new(asset_info))
        };
        drop(ret.sync().await);
        ret
    }
    pub async fn sync(&self) -> tokio::sync::RwLockReadGuard<State> {
        let mut state = self.state.write().await;
        let cursor = std::io::Cursor::new(self.remote.get_state(0).await.expect("Could not fetch state"));
        let mut buf = tokio::io::BufReader::new(cursor);
        state.replay(&mut buf).await.expect("State unable to replay");
        state.downgrade()
    }
    pub async fn apply(&self, action: tpex::Action) -> Result<u64, reqwest::Error> {
        // The remote could be desynced, so we send our update
        let id = self.remote.apply(&action).await?;
        drop(self.sync().await);
        Ok(id)
    }
}
