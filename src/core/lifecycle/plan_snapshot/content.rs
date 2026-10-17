pub(super) fn section(markdown: &str, title: &str) -> String {
    let mut output = Vec::new();
    let mut active = false;
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if active {
                break;
            }
            active = heading.trim() == title;
            continue;
        }
        if active {
            output.push(line.trim_end());
        }
    }
    output.join("\n").trim().to_owned()
}

pub(super) fn list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line)
                .trim()
                .to_owned()
        })
        .collect()
}

pub(super) fn stable_requirement_id(text: &str) -> String {
    let token = text
        .split(|character: char| character.is_whitespace() || character == ':')
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '-');
    if token.strip_prefix("REQ-").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    }) {
        token.to_owned()
    } else {
        format!("REQ-{}", &sha256(text.as_bytes())[..12])
    }
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;

pub(super) fn snapshot_uid(feature_uid: &str, content_hash: &str) -> Result<String, String> {
    let feature_uid = uuid::Uuid::parse_str(feature_uid).map_err(|error| error.to_string())?;
    use sha2::Digest as _;
    let digest =
        sha2::Sha256::digest(format!("{}:{content_hash}", feature_uid.hyphenated()).as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).hyphenated().to_string())
}

pub(super) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.hyphenated().to_string() == value)
}

pub(super) fn is_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn sha256(value: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(value))
}
