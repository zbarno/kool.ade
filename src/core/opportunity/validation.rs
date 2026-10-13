use sha2::{Digest, Sha256};

pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}
pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(super) fn is_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
pub(super) fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
pub(super) fn valid_list(values: &[String], count: usize, item_limit: usize) -> bool {
    values.len() <= count && values.iter().all(|value| valid_text(value, item_limit))
}
pub(super) fn is_koolade_remote(value: &str) -> bool {
    matches!(
        value.trim_end_matches('/'),
        "https://github.com/zbarno/kool.ade"
            | "ssh://git@github.com/zbarno/kool.ade"
            | "git@github.com:zbarno/kool.ade"
    )
}
pub(super) fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
