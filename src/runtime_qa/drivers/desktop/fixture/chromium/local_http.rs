use std::{
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) struct LocalFixture {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalFixture {
    pub(super) fn start(html: &'static str) -> Result<Self, String> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .map_err(|error| {
                format!("could not bind the private Chromium fixture page: {error}")
            })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("could not configure the private fixture page: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("could not inspect the private fixture page: {error}"))?;
        if !address.ip().is_loopback() {
            return Err("Chromium fixture page did not bind to loopback".into());
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = Arc::clone(&stopped);
        let worker = thread::Builder::new()
            .name("qa-chromium-fixture-http".into())
            .spawn(move || {
                while !thread_stopped.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, peer)) if peer.ip().is_loopback() => {
                            let _ = serve_page(stream, html);
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| format!("could not start the private fixture page: {error}"))?;
        Ok(Self {
            address,
            stopped,
            worker: Some(worker),
        })
    }

    pub(super) fn url(&self) -> String {
        format!("http://{}/", self.address)
    }
}

impl Drop for LocalFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_page(mut stream: TcpStream, html: &str) -> std::io::Result<()> {
    let body = html.as_bytes();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'none'; img-src 'none'; form-action 'none'; base-uri 'none'\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}
