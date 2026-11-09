use sha2::{Digest, Sha256};

pub(in crate::computer_use::apps) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
}

pub(super) fn valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

pub(super) fn valid_version(value: &str) -> bool {
    !value.is_empty() && value.len() <= 96 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(super) fn bounded_label(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub(in crate::computer_use::apps) fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub(super) fn decode_hex(value: &str) -> Result<Vec<u8>, &'static str> {
    if !value.len().is_multiple_of(2) {
        return Err("guest app catalog signature is malformed");
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or("guest app catalog signature is malformed")?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or("guest app catalog signature is malformed")?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}
