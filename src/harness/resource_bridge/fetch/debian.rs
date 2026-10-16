use super::{TempResponse, curl, public_address};
use std::fs;
use url::Url;

/// Retrieve only artifacts from Debian's fixed HTTPS origin; redirects fail.
pub(crate) fn retrieve_debian_artifact(path: &str, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        max_bytes > 0
            && max_bytes <= 512 * 1024 * 1024
            && (path == "dists/bookworm/InRelease"
                || (path.starts_with("dists/bookworm/main/binary-")
                    && path.ends_with("/Packages.gz"))
                || (path.starts_with("pool/main/") && path.ends_with(".deb")))
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "Debian artifact path is outside the approved repository"
    );
    let url = Url::parse(&format!("https://deb.debian.org/debian/{path}"))?;
    let temp = TempResponse::new()?;
    let address = public_address(&url)?;
    let status = curl(&url, address, &temp, max_bytes)?;
    anyhow::ensure!(
        (200..300).contains(&status),
        "Debian repository redirect or HTTP failure was rejected ({status})"
    );
    let bytes = fs::read(&temp.body)?;
    anyhow::ensure!(
        bytes.len() as u64 <= max_bytes,
        "Debian artifact exceeded its transfer limit"
    );
    Ok(bytes)
}
