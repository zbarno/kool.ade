pub(in crate::core::planning_sync) fn record_markdown_uid(path: &str) -> Option<&str> {
    let uid = path.strip_prefix("planning/items/")?.strip_suffix(".md")?;
    let parsed = uuid::Uuid::parse_str(uid).ok()?;
    (parsed.hyphenated().to_string() == uid).then_some(uid)
}

pub(super) fn validate_item_content(bytes: &[u8], expected_uid: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.len() <= 1_048_576,
        "planning item content exceeds the one-megabyte sync limit"
    );
    let content = std::str::from_utf8(bytes)?;
    crate::artifacts::items_io::validate_content_uid(content, expected_uid)
}
