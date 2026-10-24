use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use bollard::{API_DEFAULT_VERSION, BollardRequest, Docker};
use hyper::{Uri, rt::ReadBufCursor};
use hyper_util::{
    client::legacy::{
        Client,
        connect::{Connected, Connection},
    },
    rt::{TokioExecutor, TokioIo},
};
use tokio::net::UnixStream;
use tower_service::Service;

#[cfg(test)]
mod tests;

#[derive(Clone)]
struct PinnedUnixConnector {
    path: PathBuf,
    used: Arc<AtomicBool>,
}

struct PinnedUnixStream(TokioIo<UnixStream>);

impl Service<Uri> for PinnedUnixConnector {
    type Response = PinnedUnixStream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _uri: Uri) -> Self::Future {
        let path = self.path.clone();
        let used = self.used.clone();
        Box::pin(async move {
            used.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        "The approved Docker connection cannot be redirected or reopened",
                    )
                })?;
            Ok(PinnedUnixStream(TokioIo::new(
                UnixStream::connect(path).await?,
            )))
        })
    }
}

impl Connection for PinnedUnixStream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl hyper::rt::Read for PinnedUnixStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl hyper::rt::Write for PinnedUnixStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

pub(super) fn connect(path: &Path) -> Result<Docker, bollard::errors::Error> {
    let client = Arc::new(
        Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(1)
            .build(PinnedUnixConnector {
                path: path.to_owned(),
                used: Arc::new(AtomicBool::new(false)),
            }),
    );
    let transport = move |request: BollardRequest| {
        let client = Arc::clone(&client);
        Box::pin(async move {
            client
                .request(request)
                .await
                .map_err(bollard::errors::Error::from)
        })
    };
    Docker::connect_with_custom_transport(
        transport,
        // Bollard uses this logical authority to build valid HTTP request
        // URIs. The connector ignores it and dials only the pinned Unix path.
        Some("http://pinned-docker"),
        5,
        API_DEFAULT_VERSION,
    )
}
