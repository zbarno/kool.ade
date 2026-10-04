use super::{ResourceResponse, SANDBOX_RESOURCE_DIR, policy};
use std::{
    fs,
    net::{IpAddr, ToSocketAddrs},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use url::Url;

pub(super) const MAX_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INLINE_BYTES: usize = 64 * 1024;

pub(super) fn retrieve(
    worktree: &Path,
    raw_url: &str,
    purpose: &str,
    remaining_bytes: u64,
) -> anyhow::Result<ResourceResponse> {
    anyhow::ensure!(
        (3..=240).contains(&purpose.trim().len()) && !purpose.chars().any(char::is_control),
        "Explain briefly what this resource is needed for"
    );
    let mut current = match policy::classify(raw_url) {
        Ok(policy::Decision::Allow(url)) => url,
        Err(error) => {
            return Ok(ResourceResponse::needs_attention(format!(
                "The worker's resource request needs operator review: {error:#}"
            )));
        }
        Ok(policy::Decision::NeedsAttention(detail)) => {
            return Ok(ResourceResponse::needs_attention(detail));
        }
    };
    let temp = TempResponse::new()?;
    let max_bytes = MAX_RESOURCE_BYTES.min(remaining_bytes);
    anyhow::ensure!(max_bytes > 0, "Resource download budget is exhausted");
    let mut transferred = 0_u64;
    for redirect in 0..=3 {
        anyhow::ensure!(
            transferred < max_bytes,
            "Resource redirect chain exhausted its download limit"
        );
        let address = public_address(&current)?;
        let status = curl(&current, address, &temp, max_bytes - transferred)?;
        let headers = fs::read_to_string(&temp.headers)?;
        transferred += fs::metadata(&temp.body)?.len();
        anyhow::ensure!(
            transferred <= max_bytes,
            "Resource redirect chain exceeded its download limit"
        );
        if (300..400).contains(&status) {
            let Some(location) = header_value(&headers, "location") else {
                anyhow::bail!("Resource host returned a redirect without a destination");
            };
            anyhow::ensure!(redirect < 3, "Resource host redirected too many times");
            let next = current.join(&location)?;
            current = match policy::classify(next.as_str()) {
                Ok(policy::Decision::Allow(url)) => url,
                Err(error) => {
                    return Ok(ResourceResponse::needs_attention_with_bytes(
                        format!("Resource redirect needs operator review: {error:#}"),
                        transferred as usize,
                    ));
                }
                Ok(policy::Decision::NeedsAttention(detail)) => {
                    return Ok(ResourceResponse::needs_attention_with_bytes(
                        detail,
                        transferred as usize,
                    ));
                }
            };
            continue;
        }
        anyhow::ensure!(
            (200..300).contains(&status),
            "Resource host returned HTTP {status}"
        );
        let bytes = fs::read(&temp.body)?;
        let mime = header_value(&headers, "content-type").unwrap_or_default();
        let display = format!(
            "{}{}",
            current.host_str().unwrap_or_default(),
            current.path()
        );
        if !current.path().ends_with(".tgz")
            && bytes.len() <= MAX_INLINE_BYTES
            && (mime.starts_with("text/") || mime.contains("json") || mime.contains("xml"))
            && let Ok(content) = String::from_utf8(bytes.clone())
        {
            let mut response = ResourceResponse::allowed_text(
                format!("Retrieved {display} for: {}", purpose.trim()),
                content,
            );
            response.bytes = transferred as usize;
            return Ok(response);
        }
        let path = store_resource(worktree, &bytes)?;
        return Ok(ResourceResponse::allowed_file(
            format!("Retrieved {display} for: {}", purpose.trim()),
            path,
            transferred as usize,
        ));
    }
    anyhow::bail!("Resource redirect flow ended unexpectedly")
}

fn curl(url: &Url, address: IpAddr, temp: &TempResponse, max_bytes: u64) -> anyhow::Result<u16> {
    let executable = ["/usr/bin/curl", "/bin/curl", "/usr/local/bin/curl"]
        .iter()
        .map(PathBuf::from)
        .find(|path| is_executable(path))
        .ok_or_else(|| anyhow::anyhow!("The secure resource downloader curl is unavailable"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Resource URL has no host"))?;
    let pinned = match address {
        IpAddr::V4(address) => format!("{host}:443:{address}"),
        IpAddr::V6(address) => format!("{host}:443:[{address}]"),
    };
    let output = Command::new(executable)
        .env_clear()
        .args([
            "-q",
            "--silent",
            "--show-error",
            "--noproxy",
            "*",
            "--proto",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "30",
            "--max-filesize",
            &max_bytes.to_string(),
            "--resolve",
            &pinned,
            "--header",
            "Accept: application/json, application/octet-stream, text/plain, text/html",
            "--dump-header",
            temp.headers
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Temporary header path is not UTF-8"))?,
            "--output",
            temp.body
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Temporary body path is not UTF-8"))?,
            "--write-out",
            "%{http_code}",
            url.as_str(),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Resource download failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let status = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()?;
    Ok(status)
}

fn public_address(url: &Url) -> anyhow::Result<IpAddr> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Resource URL has no host"))?;
    let mut addresses = (host, 443).to_socket_addrs()?.map(|address| address.ip());
    let address = addresses
        .next()
        .ok_or_else(|| anyhow::anyhow!("Resource host did not resolve"))?;
    anyhow::ensure!(
        public_ip(address) && addresses.all(public_ip),
        "Resource host resolves to a private or reserved network address"
    );
    Ok(address)
}

fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || (a == 100 && (64..=127).contains(&b))
                || a == 127
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 88 && c == 99)
                || (a == 192 && b == 0 && c == 2)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            (segments[0] & 0xe000 == 0x2000)
                && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && ip
                    .to_ipv4_mapped()
                    .is_none_or(|mapped| public_ip(mapped.into()))
        }
    }
}

fn header_value(headers: &str, key: &str) -> Option<String> {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case(key))
        .map(|(_, value)| value.trim().to_owned())
        .next_back()
}

fn store_resource(resource_dir: &Path, bytes: &[u8]) -> anyhow::Result<String> {
    let directory = resource_dir.canonicalize()?;
    anyhow::ensure!(
        directory == resource_dir,
        "Resource cache path changed unexpectedly"
    );
    let path = directory.join(format!("{}.resource", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    use std::io::Write;
    file.write_all(bytes)?;
    Ok(format!(
        "{SANDBOX_RESOURCE_DIR}/{}.resource",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.canonicalize().is_ok_and(|path| {
        path.is_file()
            && path
                .metadata()
                .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    })
}

struct TempResponse {
    directory: PathBuf,
    headers: PathBuf,
    body: PathBuf,
}

impl TempResponse {
    fn new() -> anyhow::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "koolade-resource-response-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            headers: directory.join("headers"),
            body: directory.join("body"),
            directory,
        })
    }
}

impl Drop for TempResponse {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(test)]
mod tests;
