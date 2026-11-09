use std::path::{Component, Path};

use sha2::{Digest, Sha256};

pub(super) fn valid_relative_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.chars().any(unsafe_display_character)
    {
        return false;
    }
    let path = Path::new(value);
    if path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return false;
    }
    value.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with([' ', '.'])
            && !part
                .chars()
                .any(|ch| matches!(ch, '<' | '>' | '"' | '|' | '?' | '*'))
            && !is_reserved_device_name(part)
            && part.encode_utf16().count() <= 255
    })
}

pub(super) fn valid_display_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.encode_utf16().count() <= 255
        && !value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !value.chars().any(unsafe_display_character)
        && !value.ends_with([' ', '.'])
        && !matches!(value, "." | "..")
        && !is_reserved_device_name(value)
}

fn unsafe_display_character(character: char) -> bool {
    use unicode_general_category::{GeneralCategory, get_general_category};
    matches!(
        get_general_category(character),
        GeneralCategory::Control | GeneralCategory::Format
    )
}

pub(super) fn media_type_for_name(name: &str) -> Option<String> {
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    let media = match extension.as_str() {
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "rs" | "toml" | "yaml" | "yml" | "py" | "sh" | "ts" | "jsx" | "tsx" | "c" | "h" | "cpp"
        | "hpp" => "text/plain",
        "zip" | "tar" | "gz" | "tgz" => "application/octet-stream",
        _ => return None,
    };
    Some(media.to_owned())
}

pub(super) fn validate_content(name: &str, bytes: &[u8]) -> Result<String, &'static str> {
    let media_type = media_type_for_name(name).ok_or("file type is not approved for transfer")?;
    if bytes.is_empty() || bytes.len() as u64 > super::COMPUTER_ARTIFACT_MAX_BYTES_V1 {
        return Err("file content is empty or exceeds the transfer quota");
    }
    let header = bytes.get(..8).unwrap_or(bytes);
    let valid_signature = match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => header.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "pdf" => bytes.starts_with(b"%PDF-"),
        "zip" => bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06"),
        "gz" | "tgz" => bytes.starts_with(b"\x1f\x8b"),
        _ => true,
    };
    if !valid_signature {
        return Err("file extension does not match its content signature");
    }
    if media_type.starts_with("text/") && std::str::from_utf8(bytes).is_err() {
        return Err("text file is not valid UTF-8");
    }
    if is_archive(name) {
        validate_archive_bounds(name, bytes)?;
    }
    Ok(media_type)
}

fn validate_archive_bounds(name: &str, bytes: &[u8]) -> Result<(), &'static str> {
    let extension = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "gz" || extension == "tgz" {
        use std::io::Read;
        let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
        let mut expanded = Vec::new();
        decoder
            .take(super::COMPUTER_ARTIFACT_MAX_BYTES_V1 + 1)
            .read_to_end(&mut expanded)
            .map_err(|_| "compressed archive could not be inspected safely")?;
        if expanded.len() as u64 > super::COMPUTER_ARTIFACT_MAX_BYTES_V1 {
            return Err("compressed archive expands beyond the transfer quota");
        }
        if extension == "tgz" || looks_like_tar(&expanded) {
            validate_tar_entries(&expanded)?;
        }
    } else if extension == "tar" {
        validate_tar_entries(bytes)?;
    }
    // ZIP is deliberately kept opaque: no bridge operation extracts it, so a
    // compressed-size/expanded-size mismatch cannot allocate unbounded memory.
    Ok(())
}

fn looks_like_tar(bytes: &[u8]) -> bool {
    bytes
        .get(257..262)
        .is_some_and(|signature| signature == b"ustar")
}

fn validate_tar_entries(bytes: &[u8]) -> Result<(), &'static str> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(bytes));
    let entries = archive
        .entries()
        .map_err(|_| "archive could not be inspected safely")?;
    let mut total = 0_u64;
    let mut count = 0_u32;
    for item in entries {
        let entry = item.map_err(|_| "archive entry is malformed")?;
        count = count.saturating_add(1);
        if count > 4096 {
            return Err("archive contains too many entries");
        }
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err("archive contains links, device nodes, or unsupported entries");
        }
        let path = entry
            .path()
            .map_err(|_| "archive path is malformed")?
            .to_string_lossy()
            .into_owned();
        if !valid_relative_path(path.trim_start_matches("./")) {
            return Err("archive contains an unsafe path");
        }
        total = total.saturating_add(
            entry
                .header()
                .size()
                .map_err(|_| "archive size is malformed")?,
        );
        if total > super::COMPUTER_ARTIFACT_MAX_BYTES_V1 {
            return Err("archive expands beyond the transfer quota");
        }
    }
    Ok(())
}

pub(super) fn is_archive(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "zip" | "tar" | "tgz" | "gz"
            )
        })
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::from("sha256:");
    for byte in digest {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

fn is_reserved_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.']);
    let stem = stem.to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].into_iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(suffix, "¹" | "²" | "³")
                || (suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit() && suffix != "0")
        })
    })
}

pub(super) fn casefold_equal(left: &str, right: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    let left = left.nfd().collect::<String>();
    let right = right.nfd().collect::<String>();
    focaccia::CaseFold::Full.casecmp(&left, &right).is_eq()
}
