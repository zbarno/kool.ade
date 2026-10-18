use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) fn open_page(profile: &Path, url: &str) -> Result<(), String> {
    if !url.starts_with("http://127.0.0.1:") || url.contains(['\r', '\n', '#', '?']) {
        return Err("Chromium fixture navigation must target its local loopback page".into());
    }
    let port_file = profile.join("DevToolsActivePort");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(contents) = fs::read_to_string(&port_file)
            && let Some(line) = contents.lines().next()
            && let Ok(port) = line.parse::<u16>()
            && port != 0
        {
            match create_target(port, url) {
                Ok(()) => return Ok(()),
                Err(_error) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error),
            }
        }
        if Instant::now() >= deadline {
            return Err("Chromium did not expose its private DevTools endpoint".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn create_target(port: u16, url: &str) -> Result<(), String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("could not connect to private Chromium DevTools: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("could not configure private Chromium DevTools read: {error}"))?;
    let mut writer = stream
        .try_clone()
        .map_err(|error| format!("could not clone private Chromium DevTools socket: {error}"))?;
    write!(
        writer,
        "PUT /json/new?{} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        encode_query(url)
    )
    .map_err(|error| format!("could not send private Chromium DevTools request: {error}"))?;
    let mut stream = BufReader::new(stream);
    let mut status = String::new();
    stream
        .read_line(&mut status)
        .map_err(|error| format!("could not read private Chromium DevTools status: {error}"))?;
    let mut content_length = None;
    loop {
        let mut line = String::new();
        stream.read_line(&mut line).map_err(|error| {
            format!("could not read private Chromium DevTools headers: {error}")
        })?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    if !status.contains(" 200 ") {
        return Err("Chromium DevTools rejected the local fixture page".into());
    }
    let length = content_length
        .filter(|length| *length <= 16 * 1024)
        .ok_or_else(|| "Chromium DevTools response had an invalid content length".to_owned())?;
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("could not read private Chromium DevTools target: {error}"))?;
    let target: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| "Chromium DevTools target response was invalid")?;
    if !matches!(target["type"].as_str(), Some("page" | "tab")) || target["url"] != url {
        return Err(format!(
            "Chromium DevTools opened unexpected target type {:?} at {:?}, expected {url:?}",
            target["type"].as_str(),
            target["url"].as_str()
        ));
    }
    Ok(())
}

fn encode_query(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
