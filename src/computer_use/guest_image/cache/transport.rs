use std::time::Duration;

use reqwest::{blocking::Client, redirect::Policy};

use super::ImageCacheErrorV1;

const MAX_DOWNLOAD_REDIRECTS: usize = 5;

pub(super) fn release_tag(value: &str) -> Result<&str, ImageCacheErrorV1> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || value.contains("..")
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        || !(bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-._".contains(byte))
    {
        return Err(ImageCacheErrorV1::InvalidManifest("invalid release tag"));
    }
    Ok(value)
}

pub(super) fn release_client() -> Result<Client, ImageCacheErrorV1> {
    let policy = Policy::custom(|attempt| {
        let url = attempt.url();
        let allowed_host = matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        );
        if attempt.previous().len() >= MAX_DOWNLOAD_REDIRECTS
            || url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || !allowed_host
        {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60 * 60))
        .redirect(policy)
        .build()
        .map_err(|_| ImageCacheErrorV1::DownloadFailed)
}
