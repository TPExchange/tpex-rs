use std::pin::pin;

use tpex::Action;

use super::tokens;


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

pub(crate) struct TPExState<Stream: tokio::io::AsyncSeek + tokio::io::AsyncWrite + tokio::io::AsyncRead + Unpin> {
    state: tpex::State,
    file: Stream,
    cache: Vec<String>
}
impl<Stream: tokio::io::AsyncSeek + tokio::io::AsyncWrite + tokio::io::AsyncRead + Unpin> TPExState<Stream> {
    pub fn new(state: tpex::State, file: Stream, cache: Vec<String>) -> Self {
        Self { state, file, cache }
    }

    pub async fn apply(&mut self, action: Action) -> Result<u64, tpex::Error> {
        let mut stream = CachedFileView::new(&mut self.file);
        let ret = self.state.apply(action, &mut stream).await?;
        self.cache.push(String::from_utf8(stream.extract()).expect("Produced non-utf8 log line"));
        Ok(ret)
    }

    pub(crate) fn cache(&self) -> &[String] {
        &self.cache
    }

    pub(crate) fn state(&self) -> &tpex::State {
        &self.state
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
