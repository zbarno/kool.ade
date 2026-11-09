use super::policy::validate_public_answers;
const MAX_CONNECT_HEADER_BYTES: usize = 8 * 1024;
const MAX_TRANSFER_BYTES: u64 = 64 * 1024 * 1024;
const SOCKET_POLL: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct TunnelState {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    generation: u64,
    started: Instant,
    lifetime: Duration,
    transferred: Arc<AtomicU64>,
}

impl TunnelState {
    pub(super) fn new(
        stop: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
        epoch: Arc<AtomicU64>,
        generation: u64,
        started: Instant,
        lifetime: Duration,
        transferred: Arc<AtomicU64>,
    ) -> Self {
        Self {
            stop,
            paused,
            epoch,
            generation,
            started,
            lifetime,
            transferred,
        }
    }

    fn is_active(&self) -> bool {
        is_active(
            &self.stop,
            &self.paused,
            &self.epoch,
            self.generation,
            self.started,
            self.lifetime,
        )
    }
}

pub(super) fn serve_connection(mut client: TcpStream, tunnel: TunnelState) -> io::Result<()> {
    client.set_read_timeout(Some(SOCKET_POLL))?;
    client.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    if !tunnel.is_active() {
        return Ok(());
    }
    let header = read_connect_header(&mut client, &tunnel)?;
    let Some(host) = parse_connect_target(&header) else {
        write_rejection(&mut client, "403 Forbidden")?;
        return Ok(());
    };
    let addresses = match resolve_public_https(&host) {
        Ok(addresses) => addresses,
        Err(_) => {
            write_rejection(&mut client, "403 Forbidden")?;
            return Ok(());
        }
    };
    if !tunnel.is_active() {
        return Ok(());
    }
    let upstream = addresses
        .into_iter()
        .find_map(|address| TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).ok());
    let Some(upstream) = upstream else {
        write_rejection(&mut client, "502 Bad Gateway")?;
        return Ok(());
    };
    if !tunnel.is_active() {
        return Ok(());
    }
    upstream.set_read_timeout(Some(SOCKET_POLL))?;
    upstream.set_write_timeout(Some(SOCKET_POLL))?;
    upstream.set_nodelay(true)?;
    client.set_nodelay(true)?;
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;

    let client_reader = client.try_clone()?;
    let upstream_writer = upstream.try_clone()?;
    let connection_stop = Arc::new(AtomicBool::new(false));
    let upload_connection_stop = connection_stop.clone();
    let upload_tunnel = tunnel.clone();
    let upload = thread::spawn(move || {
        relay(
            client_reader,
            upstream_writer,
            upload_tunnel,
            upload_connection_stop,
        )
    });
    let download = relay(upstream, client, tunnel, connection_stop);
    let _ = upload.join();
    download?;
    Ok(())
}

fn relay<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    tunnel: TunnelState,
    connection_stop: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut buffer = [0_u8; 16 * 1024];
    let result = loop {
        if connection_stop.load(Ordering::Acquire) || !tunnel.is_active() {
            break Ok(());
        }
        match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(size) => {
                if connection_stop.load(Ordering::Acquire) || !tunnel.is_active() {
                    break Ok(());
                }
                let reservation = tunnel.transferred.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                    |current| {
                        current
                            .checked_add(size as u64)
                            .filter(|next| *next <= MAX_TRANSFER_BYTES)
                    },
                );
                if reservation.is_err() {
                    break Err(io::Error::other(
                        "InternetResearch transfer budget exceeded",
                    ));
                }
                if connection_stop.load(Ordering::Acquire) || !tunnel.is_active() {
                    break Ok(());
                }
                if let Err(error) = writer.write_all(&buffer[..size]) {
                    break Err(error);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Err(error),
        }
    };
    connection_stop.store(true, Ordering::Release);
    result
}

fn read_connect_header(stream: &mut TcpStream, tunnel: &TunnelState) -> io::Result<String> {
    let mut header = Vec::with_capacity(512);
    let mut byte = [0_u8; 1];
    while header.len() < MAX_CONNECT_HEADER_BYTES {
        if !tunnel.is_active() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "gateway expired"));
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) => {
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    return String::from_utf8(header)
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid header"));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "CONNECT request header exceeds limit",
    ))
}

fn is_active(
    stop: &AtomicBool,
    paused: &AtomicBool,
    epoch: &AtomicU64,
    generation: u64,
    started: Instant,
    lifetime: Duration,
) -> bool {
    !stop.load(Ordering::Acquire)
        && !paused.load(Ordering::Acquire)
        && epoch.load(Ordering::Acquire) == generation
        && started.elapsed() < lifetime
}

pub(super) fn parse_connect_target(header: &str) -> Option<String> {
    let mut lines = header.split("\r\n");
    let request = lines.next()?;
    let fields = request.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != "CONNECT" || fields[2] != "HTTP/1.1" {
        return None;
    }
    let authority = fields[1];
    let host = authority.strip_suffix(":443")?;
    if host.is_empty()
        || host.len() > 253
        || host.parse::<IpAddr>().is_ok()
        || host.starts_with('[')
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
    let parsed = url::Url::parse(&format!("https://{host}/")).ok()?;
    if !parsed.host_str()?.eq_ignore_ascii_case(host) {
        return None;
    }
    let mut host_header_seen = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("proxy-authorization") {
            return None;
        }
        if name.eq_ignore_ascii_case("host") {
            if host_header_seen
                || !(value.trim().eq_ignore_ascii_case(authority)
                    || value.trim().eq_ignore_ascii_case(host))
            {
                return None;
            }
            host_header_seen = true;
        }
    }
    host_header_seen.then(|| host.to_ascii_lowercase())
}

fn resolve_public_https(host: &str) -> io::Result<Vec<SocketAddr>> {
    let addresses = (host, 443).to_socket_addrs()?.collect::<Vec<SocketAddr>>();
    validate_public_answers(&addresses)?;
    Ok(addresses)
}

fn write_rejection(stream: &mut TcpStream, status: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
}
