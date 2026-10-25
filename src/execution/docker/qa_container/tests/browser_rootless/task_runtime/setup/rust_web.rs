use std::{fs, path::Path};

pub(super) fn write_rust_web_app(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"runtime-qa-rust-web\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"runtime-qa-rust-web\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/index.html"), page()).unwrap();
    fs::write(root.join("src/main.rs"), RUST_SERVER).unwrap();
}

fn page() -> &'static str {
    r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Task runtime fixture</title></head>
<body><main><h1>Task runtime fixture</h1>
<form id="message-form"><label for="message">Message</label><input id="message" name="message" autocomplete="off"></form>
<p data-testid="status">Waiting</p></main>
<script>
document.querySelector("#message-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const response = await fetch("/save", { method: "POST", body: document.querySelector("#message").value });
  document.querySelector("[data-testid=status]").textContent = response.ok && await response.text() === "Saved" ? "Saved" : "Save failed";
});
</script></body></html>
"##
}

const RUST_SERVER: &str = r#"
use std::{env, io::{Read, Write}, net::{TcpListener, TcpStream}};

const PAGE: &str = include_str!("index.html");

fn main() {
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port = env::var("PORT").unwrap_or_else(|_| "4173".into());
    let listener = TcpListener::bind(format!("{host}:{port}")).unwrap();
    for stream in listener.incoming().flatten() {
        let _ = respond(stream);
    }
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    let request = read_request(&mut stream)?;
    let mut lines = request.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default();
    let path = first.next().unwrap_or_default();
    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    let (status, content_type, response) = match (method, path) {
        ("GET", "/") => (200, "text/html; charset=utf-8", PAGE.to_owned()),
        ("GET", "/health") => (200, "text/plain", "healthy".to_owned()),
        ("POST", "/save") if !body.trim().is_empty() => (200, "text/plain", "Saved".to_owned()),
        ("POST", "/save") => (400, "text/plain", "Message required".to_owned()),
        _ => (404, "text/plain", "Not found".to_owned()),
    };
    write!(stream, "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len())
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 { break; }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
            }).unwrap_or_default();
            if bytes.len() >= end + 4 + content_length { break; }
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{page, PAGE};

    #[test]
    fn page_has_the_browser_driver_contract() {
        assert!(page().contains("<label for=\"message\">Message</label>"));
        assert!(page().contains("data-testid=\"status\""));
        assert!(!PAGE.is_empty());
    }
}
"#;
