//! Provider requests are relayed by Koolade so planning Pi never receives the
//! provider credential or host network access.
use std::{
    fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

mod config;
#[cfg(test)]
mod tests;

pub(crate) fn configuration_error() -> Option<String> {
    config::provider_error()
}

pub(crate) fn configured_models() -> anyhow::Result<Vec<String>> {
    config::configured_models()
}

pub(crate) fn configured_default_model() -> anyhow::Result<String> {
    config::configured_default_model()
}

pub(super) struct ProviderBridge {
    pub socket_dir: PathBuf,
    pub agent_dir: PathBuf,
    pub port: u16,
    pub model_args: Vec<String>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    temp: PathBuf,
}

impl ProviderBridge {
    pub fn start(model: Option<&str>) -> anyhow::Result<Self> {
        let config = config::load(model)?;
        let temp = std::env::temp_dir().join(format!(
            "koolade-planning-provider-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let socket_dir = temp.join("socket");
        let agent_dir = temp.join("agent");
        fs::create_dir(&temp)?;
        set_private_dir(&temp)?;
        fs::create_dir(&socket_dir)?;
        fs::create_dir(&agent_dir)?;
        set_private_dir(&socket_dir)?;
        set_private_dir(&agent_dir)?;
        config.write_sandbox_files(&agent_dir)?;
        let socket = socket_dir.join("provider.sock");
        let listener = UnixListener::bind(&socket)?;
        set_private_file(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let target = config.target;
        let key = Arc::new(config.key);
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let target = target.clone();
                        let key = key.clone();
                        thread::spawn(move || {
                            let _ = relay(client, target, &key);
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            socket_dir,
            agent_dir,
            port: config.port,
            model_args: config.model_args,
            stop,
            worker: Some(worker),
            temp,
        })
    }

    pub fn sandbox_mounts(&self) -> Vec<String> {
        sandbox_mounts(&self.socket_dir, &self.agent_dir)
    }
}

fn sandbox_mounts(socket_dir: &Path, agent_dir: &Path) -> Vec<String> {
    [
        "--dir".into(),
        "/run/koolade-provider".into(),
        "--ro-bind".into(),
        socket_dir.to_string_lossy().into_owned(),
        "/run/koolade-provider".into(),
        "--dir".into(),
        "/run/koolade-provider-config".into(),
        "--ro-bind".into(),
        agent_dir.to_string_lossy().into_owned(),
        "/run/koolade-provider-config/agent".into(),
        "--dir".into(),
        "/tmp/koolade-home/.pi".into(),
        "--dir".into(),
        "/tmp/koolade-home/.pi/agent".into(),
    ]
    .into()
}

impl Drop for ProviderBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.temp);
    }
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[derive(Clone)]
struct Target {
    address: SocketAddr,
    host: String,
    prefix: String,
}

fn relay(mut client: UnixStream, target: Target, key: &str) -> io::Result<()> {
    let mut header = Vec::with_capacity(4096);
    let mut byte = [0; 1];
    while header.len() < 65_536 {
        if client.read(&mut byte)? == 0 {
            return Ok(());
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    if !header.ends_with(b"\r\n\r\n") {
        return write_rejection(&mut client, 431, "Request headers too large");
    }
    let parsed = match rewrite_header(&header, &target, key) {
        Some(header) => header,
        None => return write_rejection(&mut client, 403, "Provider route rejected"),
    };
    let mut upstream = match TcpStream::connect_timeout(&target.address, Duration::from_secs(5)) {
        Ok(stream) => stream,
        Err(_) => return write_rejection(&mut client, 502, "Provider unavailable"),
    };
    set_idle_read_timeouts(&upstream, &client)?;
    upstream.set_write_timeout(Some(Duration::from_secs(90)))?;
    client.set_write_timeout(Some(Duration::from_secs(90)))?;
    upstream.write_all(&parsed)?;
    let mut upload = client.try_clone()?;
    let mut upstream_upload = upstream.try_clone()?;
    let sender = thread::spawn(move || {
        let _ = io::copy(&mut upload, &mut upstream_upload);
        let _ = upstream_upload.shutdown(std::net::Shutdown::Write);
    });
    let _ = io::copy(&mut upstream, &mut client);
    let _ = sender.join();
    Ok(())
}

fn set_idle_read_timeouts(upstream: &TcpStream, client: &UnixStream) -> io::Result<()> {
    let timeout = Some(crate::harness::pi_harness::configured_stall_timeout());
    upstream.set_read_timeout(timeout)?;
    client.set_read_timeout(timeout)
}

fn rewrite_header(header: &[u8], target: &Target, key: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(header).ok()?;
    let mut lines = text.split("\r\n");
    let request = lines.next()?;
    let mut request_parts = request.split_whitespace();
    let method = request_parts.next()?;
    let path = request_parts.next()?;
    if method != "POST"
        || request_parts.next()? != "HTTP/1.1"
        || !path.starts_with(&target.prefix)
        || path.contains("..")
        || key.contains(['\r', '\n'])
    {
        return None;
    }
    let mut output = format!("{request}\r\n");
    let mut host_seen = false;
    let mut auth_seen = false;
    let mut lengths = 0;
    let mut transfer_encoding = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        match name.to_ascii_lowercase().as_str() {
            "host" => {
                if host_seen {
                    return None;
                }
                host_seen = true;
                output.push_str(&format!("Host: {}\r\n", target.host));
            }
            "authorization" => {
                if auth_seen {
                    return None;
                }
                auth_seen = true;
                output.push_str(&format!("Authorization: Bearer {key}\r\n"));
            }
            "content-length" => {
                lengths += 1;
                if lengths > 1 || value.trim().parse::<u64>().is_err() {
                    return None;
                }
                output.push_str(line);
                output.push_str("\r\n");
            }
            "transfer-encoding" => {
                if transfer_encoding || value.trim().eq_ignore_ascii_case("identity") {
                    return None;
                }
                transfer_encoding = true;
                output.push_str(line);
                output.push_str("\r\n");
            }
            "connection" | "proxy-connection" => {}
            _ => {
                output.push_str(line);
                output.push_str("\r\n");
            }
        }
    }
    if !host_seen || lengths > 0 && transfer_encoding {
        return None;
    }
    if !auth_seen {
        output.push_str(&format!("Authorization: Bearer {key}\r\n"));
    }
    output.push_str("Connection: close\r\n\r\n");
    Some(output.into_bytes())
}

fn write_rejection(stream: &mut UnixStream, status: u16, message: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} Rejected\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}",
        message.len()
    )
}

pub(super) const SANDBOX_RELAY: &str = "const net=require('node:net'),fs=require('node:fs');const server=net.createServer(c=>{const s=net.createConnection({path:'/run/koolade-provider/provider.sock'});c.pipe(s);s.pipe(c);});server.listen(Number(process.argv[2]),'127.0.0.1',()=>fs.writeFileSync('/tmp/koolade-model-relay-ready','ready'));";

pub(super) const BOOTSTRAP: &str = r#"
set -e
config=/run/koolade-provider-config/agent
runtime=/tmp/koolade-home/.pi/agent
cp "$config/settings.json" "$runtime/settings.json"
cp "$config/models.json" "$runtime/models.json"
cp "$config/model-relay.cjs" "$runtime/model-relay.cjs"
node "$runtime/model-relay.cjs" "$1" &
relay=$!
trap 'kill "$relay" 2>/dev/null || :' EXIT
i=0
while [ ! -e /tmp/koolade-model-relay-ready ]; do
    i=$((i + 1))
    [ "$i" -lt 100 ] || exit 70
    sleep 0.01
done
shift
exec "$@"
"#;
