use std::pin::pin;

use hashbrown::HashMap;

use tpex::{ids::HashMapCowExt, Action};

use super::{PriceSummary, tokens};

use tokio::io::AsyncBufReadExt;


struct CachedFileView<Stream: tokio::io::AsyncWrite> {
    base: Stream,
    cache: Vec<u8>
}
impl<Stream: tokio::io::AsyncWrite> CachedFileView<Stream> {
    fn new(base: Stream) -> Self {
        CachedFileView { base, cache: Vec::new() }
    }
    fn extract(self) -> Vec<u8> {
        self.cache
    }
}
impl<Stream: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for CachedFileView<Stream> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let ret = pin!(&mut self.base).poll_write(cx, buf);
        if let std::task::Poll::Ready(Ok(len)) = ret {
            self.cache.extend_from_slice(&buf[..len]);
        }
        ret
    }

    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        pin!(&mut self.base).poll_flush(cx)
    }

    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        pin!(&mut self.base).poll_shutdown(cx)
    }
}

pub(crate) struct TPExState<Stream: tokio::io::AsyncWrite> {
    state: tpex::State,
    file: Stream,
    cache: Vec<String>,
    price_history: HashMap<tpex::AssetId<'static>, Vec<PriceSummary>>
}
impl<Stream: tokio::io::AsyncSeek + tokio::io::AsyncWrite + tokio::io::AsyncRead + Unpin + tokio::io::AsyncBufRead> TPExState<Stream> {
    pub async fn replay(file: Stream) -> Result<Self, tpex::Error> {
        // This is the state we will call apply on repeatedly
        //
        // When we're done, we'll extract all the information and add in the file, which will now be positioned at the end
        let mut tmp_state = TPExState { state: tpex::State::new(), file: tokio::io::sink(), cache: Default::default(), price_history: Default::default() };
        let mut lines = file.lines();
        while let Some(line) = lines.next_line().await.expect("Could not read next action") {
            let wrapped_action: tpex::WrappedAction = serde_json::from_str(&line).expect("Could not parse state");
            let id = tmp_state.apply(wrapped_action.action, wrapped_action.time).await?;
            assert_eq!(id, wrapped_action.id, "Wrapped action had out-of-order id");
        }
        Ok(Self {
            file: lines.into_inner(),
            state: tmp_state.state,
            cache: tmp_state.cache,
            price_history: tmp_state.price_history
        })
    }
}
impl<Stream: tokio::io::AsyncWrite + Unpin> TPExState<Stream> {
    #[allow(dead_code)]
    pub fn new(file: Stream, cache: Vec<String>) -> Self {
        TPExState { state: tpex::State::new(), file, cache, price_history: Default::default() }
    }

    pub async fn apply<'a>(&mut self, action: Action<'a>, time: chrono::DateTime<chrono::Utc>) -> Result<u64, tpex::Error> {
        // Grab the information to price history before we consume the action and modify everything
        let maybe_asset = match &action {
            tpex::Action::BuyOrder { asset, .. } => Some(asset.clone()),
            tpex::Action::SellOrder { asset, .. } => Some(asset.clone()),
            tpex::Action::CancelOrder { target } => Some(self.state.get_order(*target).expect("Invalid order id").asset.clone()),
            _ => None
        };

        let mut stream = CachedFileView::new(&mut self.file);
        let ret = self.state.apply_with_time(action, time, &mut stream).await?;
        // If the price has changed, log it
        if let Some(asset) = maybe_asset {
            let (new_buy, new_sell) = self.state.get_prices(&asset);
            let new_elem = PriceSummary {
                time,
                best_buy: new_buy.keys().next_back().cloned(),
                n_buy: new_buy.values().sum(),
                best_sell: new_sell.keys().next().cloned(),
                n_sell: new_sell.values().sum()
            };
            let target = self.price_history.cow_get_or_default(asset).1;
            target.push(new_elem);
        }
        self.cache.push(String::from_utf8(stream.extract()).expect("Produced non-utf8 log line"));
        Ok(ret)
    }

    pub fn cache(&self) -> &[String] {
        &self.cache
    }

    pub fn state(&self) -> &tpex::State {
        &self.state
    }

    pub fn price_history(&self) -> &HashMap<tpex::AssetId, Vec<PriceSummary>> {
        &self.price_history
    }
    // async fn get_lines(&mut self) -> Vec<u8> {
    //     // Keeping everything in the log file means we can't have different versions of the same data
    //     self.file.rewind().await.expect("Could not rewind trade file.");
    //     let mut buf = Vec::new();
    //     // This will seek to the end again, so pos is the same before and after get_lines
    //     self.file.read_to_end(&mut buf).await.expect("Could not re-read trade file.");
    //     buf
    // }
}

pub(crate) struct StateStruct<Stream: tokio::io::AsyncSeek + tokio::io::AsyncWrite + tokio::io::AsyncRead + Unpin> {
    pub(crate) tpex: tokio::sync::RwLock<TPExState<Stream>>,
    pub(crate) tokens: tokens::TokenHandler,
    pub(crate) updated: tokio::sync::watch::Sender<u64>,
}
#[macro_export]
macro_rules! state_type {
    () => {
        std::sync::Arc<$crate::server::state::StateStruct<impl AsyncBufRead + AsyncWrite + AsyncSeek + Unpin + Send + Sync + 'static>>
    };
}
