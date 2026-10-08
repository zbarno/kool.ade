//! Loopback HTTPS tunnel for bounded host package-manager requests.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const MAX_CONNECT_HEADER: usize = 8 * 1024;
const MAX_CONNECTIONS: usize = 16;

struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(super) struct HttpsRegistryProxy {
    address: String,
    bytes_received: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HttpsRegistryProxy {
    pub(super) fn start(
        allowed_hosts: impl IntoIterator<Item = String>,
        max_bytes: usize,
    ) -> anyhow::Result<Self> {
        let allowed_hosts = allowed_hosts.into_iter().collect::<Vec<_>>();
        anyhow::ensure!(
            !allowed_hosts.is_empty(),
            "Registry proxy requires an explicit host allowlist"
        );
        anyhow::ensure!(max_bytes > 0, "Registry proxy download budget is exhausted");
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_hosts = Arc::new(allowed_hosts);
        let bytes_received = Arc::new(AtomicUsize::new(0));
        let worker_bytes_received = bytes_received.clone();
        let active_connections = Arc::new(AtomicUsize::new(0));
        let worker_active_connections = active_connections.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        if !acquire_connection_slot(&worker_active_connections) {
                            drop(client);
                            continue;
                        }
                        let bytes_received = worker_bytes_received.clone();
                        let allowed_hosts = worker_hosts.clone();
                        let active_connections = worker_active_connections.clone();
                        let spawned = thread::Builder::new()
                            .name("koolade-registry-tunnel".into())
                            .spawn(move || {
                                let _slot = ConnectionSlot(active_connections);
                                let _ = tunnel_registry_connection(
                                    client,
                                    &allowed_hosts,
                                    bytes_received,
                                    max_bytes,
                                );
                            });
                        if spawned.is_err() {
                            worker_active_connections.fetch_sub(1, Ordering::Release);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            bytes_received,
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(super) fn bytes_received(&self) -> usize {
        self.bytes_received.load(Ordering::Relaxed)
    }
}

fn acquire_connection_slot(active: &AtomicUsize) -> bool {
    active
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |count| {
            (count < MAX_CONNECTIONS).then_some(count + 1)
        })
        .is_ok()
}

impl Drop for HttpsRegistryProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn tunnel_registry_connection(
    mut client: TcpStream,
    allowed_hosts: &[String],
    bytes_received: Arc<AtomicUsize>,
    max_bytes: usize,
) -> anyhow::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = read_connect_header(&mut client)?;
    let host = allowed_connect_target(&request, allowed_hosts).ok_or_else(|| {
        anyhow::anyhow!("Package registry proxy rejected a host outside its allowlist")
    })?;
    let url = url::Url::parse(&format!("https://{host}/"))?;
    let address = super::fetch::public_address(&url)?;
    let registry =
        TcpStream::connect_timeout(&SocketAddr::new(address, 443), Duration::from_secs(10))?;
    client.set_nodelay(true)?;
    registry.set_nodelay(true)?;
    client.set_read_timeout(None)?;
    registry.set_read_timeout(Some(Duration::from_secs(45)))?;
    registry.set_write_timeout(Some(Duration::from_secs(45)))?;
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;

    let mut client_reader = client.try_clone()?;
    let mut registry_writer = registry.try_clone()?;
    let upload = thread::spawn(move || {
        let _ = io::copy(&mut client_reader, &mut registry_writer);
        let _ = registry_writer.shutdown(Shutdown::Write);
    });
    let mut registry_reader = registry;
    let mut client_writer = client;
    let downloaded = copy_download_limited(
        &mut registry_reader,
        &mut client_writer,
        &bytes_received,
        max_bytes,
    );
    let _ = client_writer.shutdown(Shutdown::Write);
    let _ = upload.join();
    downloaded?;
    Ok(())
}

fn copy_download_limited<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    transferred: &AtomicUsize,
    limit: usize,
) -> io::Result<u64> {
    let mut buffer = [0_u8; 16 * 1024];
    let mut copied = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(copied);
        }
        let reserved = transferred.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(read).filter(|next| *next <= limit)
        });
        if reserved.is_err() {
            return Err(io::Error::other("registry proxy download budget exceeded"));
        }
        writer.write_all(&buffer[..read])?;
        copied = copied.saturating_add(read as u64);
    }
}

fn read_connect_header(stream: &mut TcpStream) -> anyhow::Result<String> {
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    while header.len() < MAX_CONNECT_HEADER {
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            return String::from_utf8(header).map_err(anyhow::Error::from);
        }
    }
    anyhow::bail!("npm registry proxy received an oversized CONNECT header")
}

fn allowed_connect_target<'a>(header: &str, allowed_hosts: &'a [String]) -> Option<&'a str> {
    let mut lines = header.split("\r\n");
    let request = lines.next()?;
    let fields = request.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != "CONNECT" || fields[2] != "HTTP/1.1" {
        return None;
    }
    let host = fields[1].strip_suffix(":443")?;
    let allowed = allowed_hosts
        .iter()
        .find(|allowed| host.eq_ignore_ascii_case(allowed))?;
    let mut host_seen = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("host") {
                if host_seen || !value.trim().eq_ignore_ascii_case(fields[1]) {
                    return None;
                }
                host_seen = true;
            }
            if name.eq_ignore_ascii_case("proxy-authorization") {
                return None;
            }
        }
    }
    host_seen.then_some(allowed.as_str())
}

#[cfg(test)]
mod tests {
    use super::{ConnectionSlot, MAX_CONNECTIONS, acquire_connection_slot, allowed_connect_target};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NPM: &[&str] = &["registry.npmjs.org"];
    const CARGO: &[&str] = &["index.crates.io", "static.crates.io"];

    fn strings(hosts: &[&str]) -> Vec<String> {
        hosts.iter().map(|host| (*host).to_owned()).collect()
    }

    #[test]
    fn registry_proxy_only_accepts_the_public_npm_registry_authority() {
        let custom_registry = vec!["packages.example.net".to_owned()];
        assert_eq!(
            allowed_connect_target(
                "CONNECT registry.npmjs.org:443 HTTP/1.1\r\nHost: registry.npmjs.org:443\r\n\r\n",
                &strings(NPM),
            ),
            Some("registry.npmjs.org")
        );
        assert_eq!(
            allowed_connect_target(
                "CONNECT static.crates.io:443 HTTP/1.1\r\nHost: static.crates.io:443\r\n\r\n",
                &strings(CARGO),
            ),
            Some("static.crates.io")
        );
        assert_eq!(
            allowed_connect_target(
                "CONNECT packages.example.net:443 HTTP/1.1\r\nHost: packages.example.net:443\r\n\r\n",
                &custom_registry,
            ),
            Some("packages.example.net")
        );
        for request in [
            "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
            "CONNECT registry.npmjs.org:444 HTTP/1.1\r\nHost: registry.npmjs.org:444\r\n\r\n",
            "GET https://registry.npmjs.org/pkg HTTP/1.1\r\n\r\n",
            "CONNECT registry.npmjs.org:443 HTTP/1.1\r\nProxy-Authorization: Basic abc\r\n\r\n",
        ] {
            assert_eq!(
                allowed_connect_target(request, &strings(NPM)),
                None,
                "{request}"
            );
        }
        assert_eq!(
            allowed_connect_target(
                "CONNECT static.crates.io:443 HTTP/1.1\r\nHost: static.crates.io:443\r\n\r\n",
                &strings(NPM),
            ),
            None
        );
    }

    #[test]
    fn registry_proxy_caps_connections_and_releases_slots() {
        let active = std::sync::Arc::new(AtomicUsize::new(0));
        let slots = (0..MAX_CONNECTIONS)
            .map(|_| {
                assert!(acquire_connection_slot(&active));
                ConnectionSlot(active.clone())
            })
            .collect::<Vec<_>>();
        assert!(!acquire_connection_slot(&active));
        drop(slots);
        assert_eq!(active.load(Ordering::Relaxed), 0);
        assert!(acquire_connection_slot(&active));
    }
}
