//! Signal stdin EOF before rmcp begins draining outstanding handlers.
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, ReadBuf},
    sync::watch,
};

pub(super) struct Input<R> {
    pub reader: R,
    pub closed: watch::Sender<bool>,
}
impl<R: AsyncRead + Unpin> AsyncRead for Input<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let capacity = buf.remaining();
        let result = Pin::new(&mut self.reader).poll_read(cx, buf);
        if capacity > 0 && matches!(result, Poll::Ready(Ok(()))) && buf.filled().len() == before {
            self.closed.send_replace(true);
        }
        result
    }
}
