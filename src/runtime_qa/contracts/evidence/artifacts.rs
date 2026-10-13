use sha2::Digest;

use super::QaScreenshotV1;

/// Image bytes collected by the app-owned driver before report persistence.
pub struct QaScreenshotArtifactV1 {
    pub id: String,
    pub bytes: Vec<u8>,
}

pub fn validate_screenshot_artifact(
    screenshot: &QaScreenshotV1,
    bytes: &[u8],
    max_bytes: u64,
) -> Result<(), String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if screenshot.mime != "image/png"
        || bytes.len() < 24
        || bytes.len() as u64 > max_bytes
        || bytes.get(..8) != Some(PNG_SIGNATURE.as_slice())
        || bytes.get(12..16) != Some(b"IHDR")
    {
        return Err("runtime QA screenshot is missing or has invalid PNG data".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("checked PNG header"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("checked PNG header"));
    if width != u32::from(screenshot.width)
        || height != u32::from(screenshot.height)
        || u64::from(width).saturating_mul(u64::from(height)) > 33_177_600
    {
        return Err(
            "runtime QA screenshot dimensions exceed or disagree with their metadata".into(),
        );
    }
    let decoded = image::load_from_memory(bytes)
        .map_err(|_| "runtime QA screenshot PNG cannot be decoded")?;
    if decoded.width() != width || decoded.height() != height {
        return Err("runtime QA screenshot decoded dimensions do not match its metadata".into());
    }
    let actual_hash = format!("{:x}", sha2::Sha256::digest(bytes));
    if screenshot.sha256 != actual_hash {
        return Err("runtime QA screenshot hash does not match its bytes".into());
    }
    Ok(())
}
