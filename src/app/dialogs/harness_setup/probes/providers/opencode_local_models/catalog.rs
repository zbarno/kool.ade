use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};

use serde_json::Value;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_millis(250);
const TOTAL_IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 8 * 1024;

pub(super) enum ModelCatalogProbe {
    Available(BTreeSet<String>),
    AuthenticationRequired,
    Unavailable,
}

/// Accept only explicit direct loopback HTTP OpenAI-compatible `/v1` endpoints. Requests never
/// follow redirects, resolve hostnames, attach credentials, or use a configured proxy.
pub(super) fn configured_loopback_url(raw: &str) -> Option<url::Url> {
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    if url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost"))
    {
        let mut url = url;
        url.set_host(Some("127.0.0.1")).ok()?;
        return Some(url);
    }
    let host = loopback_ip(&url)?;
    (host.is_loopback() && url.port_or_known_default().is_some()).then_some(url)
}

pub(super) fn explicit_loopback_catalog_url(raw: &str) -> Option<url::Url> {
    let url = configured_loopback_url(raw)?;
    (url.scheme() == "http"
        && url.port_or_known_default().is_some()
        && url.path().trim_end_matches('/').ends_with("/v1"))
    .then_some(url)
}

pub(super) fn probe_model_catalog(base_url: &url::Url) -> ModelCatalogProbe {
    let mut catalog_url = base_url.clone();
    let path = format!("{}/models", base_url.path().trim_end_matches('/'));
    catalog_url.set_path(&path);
    let body = match request_model_catalog(&catalog_url) {
        Ok(body) => body,
        Err(CatalogFailure::AuthenticationRequired) => {
            return ModelCatalogProbe::AuthenticationRequired;
        }
        Err(CatalogFailure::Unavailable) => return ModelCatalogProbe::Unavailable,
    };
    let Ok(response) = serde_json::from_slice::<Value>(&body) else {
        return ModelCatalogProbe::Unavailable;
    };
    let Some(models) = response.get("data").and_then(Value::as_array) else {
        return ModelCatalogProbe::Unavailable;
    };
    ModelCatalogProbe::Available(
        models
            .iter()
            .filter_map(|model| model.get("id")?.as_str())
            .filter(|id| !id.trim().is_empty() && !id.chars().any(char::is_control))
            .map(str::to_owned)
            .collect(),
    )
}

enum CatalogFailure {
    AuthenticationRequired,
    Unavailable,
}

fn request_model_catalog(url: &url::Url) -> Result<Vec<u8>, CatalogFailure> {
    let address = SocketAddr::new(
        loopback_ip(url).ok_or(CatalogFailure::Unavailable)?,
        url.port_or_known_default()
            .ok_or(CatalogFailure::Unavailable)?,
    );
    if !address.ip().is_loopback() {
        return Err(CatalogFailure::Unavailable);
    }
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|_| CatalogFailure::Unavailable)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| CatalogFailure::Unavailable)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| CatalogFailure::Unavailable)?;
    let host = match address.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", address.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", address.port()),
    };
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        url.path()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| CatalogFailure::Unavailable)?;

    let deadline = Instant::now()
        .checked_add(TOTAL_IO_TIMEOUT)
        .ok_or(CatalogFailure::Unavailable)?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if response.len() >= MAX_RESPONSE_BYTES {
            return Err(CatalogFailure::Unavailable);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(CatalogFailure::Unavailable)?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT.min(remaining)))
            .map_err(|_| CatalogFailure::Unavailable)?;
        let read_limit = (MAX_RESPONSE_BYTES - response.len()).min(chunk.len());
        match stream.read(&mut chunk[..read_limit]) {
            Ok(0) => break,
            Ok(bytes_read) => response.extend_from_slice(&chunk[..bytes_read]),
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline =>
            {
                continue;
            }
            Err(_) => return Err(CatalogFailure::Unavailable),
        }
        if response_has_complete_content_length(&response) == Some(true) {
            break;
        }
    }
    let status = parse_http_status(&response).ok_or(CatalogFailure::Unavailable)?;
    if matches!(status, 401 | 403) {
        return Err(CatalogFailure::AuthenticationRequired);
    }
    if status != 200 {
        return Err(CatalogFailure::Unavailable);
    }
    parse_http_body(&response)
        .map(<[u8]>::to_vec)
        .ok_or(CatalogFailure::Unavailable)
}

fn loopback_ip(url: &url::Url) -> Option<IpAddr> {
    let host = url.host_str()?;
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .parse::<IpAddr>()
        .ok()
}

fn response_has_complete_content_length(response: &[u8]) -> Option<bool> {
    let header_end = find_header_end(response)?;
    if header_end > MAX_HEADER_BYTES {
        return Some(false);
    }
    let headers = std::str::from_utf8(&response[..header_end]).ok()?;
    let mut content_length = None;
    for line in headers.split("\r\n").skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>().ok()?;
            if parsed > MAX_RESPONSE_BYTES || content_length.replace(parsed).is_some() {
                return Some(false);
            }
        }
    }
    let Some(content_length) = content_length else {
        return Some(false);
    };
    Some(response.len().saturating_sub(header_end + 4) >= content_length)
}

fn parse_http_body(response: &[u8]) -> Option<&[u8]> {
    let header_end = find_header_end(response)?;
    if header_end > MAX_HEADER_BYTES {
        return None;
    }
    let headers = std::str::from_utf8(&response[..header_end]).ok()?;
    let mut lines = headers.split("\r\n");
    let status = lines.next()?.split_whitespace().collect::<Vec<_>>();
    if status.len() < 2 || !matches!(status[0], "HTTP/1.0" | "HTTP/1.1") || status[1] != "200" {
        return None;
    }
    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-encoding")
                && !value.trim().eq_ignore_ascii_case("identity"))
        {
            return None;
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>().ok()?;
            if parsed > MAX_RESPONSE_BYTES || content_length.replace(parsed).is_some() {
                return None;
            }
        }
    }
    let body = &response[header_end + 4..];
    if body.len() > MAX_RESPONSE_BYTES
        || content_length.is_some_and(|expected| expected != body.len())
    {
        return None;
    }
    Some(body)
}

fn parse_http_status(response: &[u8]) -> Option<u16> {
    let header_end = find_header_end(response)?;
    if header_end > MAX_HEADER_BYTES {
        return None;
    }
    let headers = std::str::from_utf8(&response[..header_end]).ok()?;
    let mut status = headers.split("\r\n").next()?.split_whitespace();
    if !matches!(status.next()?, "HTTP/1.0" | "HTTP/1.1") {
        return None;
    }
    status.next()?.parse().ok()
}

fn find_header_end(response: &[u8]) -> Option<usize> {
    response.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
