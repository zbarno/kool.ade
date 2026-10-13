pub(in crate::core::planning_sync) fn record_markdown_uid(path: &str) -> Option<&str> {
    let uid = path
        .strip_prefix("planning/items/")
        .or_else(|| path.strip_prefix("planning/questions/"))?
        .strip_suffix(".md")?;
    let parsed = uuid::Uuid::parse_str(uid).ok()?;
    (parsed.hyphenated().to_string() == uid).then_some(uid)
}

pub(super) fn validate_question_content(bytes: &[u8], expected_uid: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.len() <= 1_048_576,
        "planning question content exceeds the one-megabyte sync limit"
    );
    let content = std::str::from_utf8(bytes)?;
    anyhow::ensure!(
        content.contains(&format!("UID: {expected_uid}")),
        "question content UID does not match its path"
    );
    anyhow::ensure!(
        content.contains("- Feature: ") && content.contains("## Why this decision matters"),
        "question content is missing its feature identity or decision context"
    );
    Ok(())
}

pub(super) fn validate_item_content(bytes: &[u8], expected_uid: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.len() <= 1_048_576,
        "planning item content exceeds the one-megabyte sync limit"
    );
    let content = std::str::from_utf8(bytes)?;
    if content.contains("- Feature: ") && content.contains("## Why this decision matters") {
        return validate_question_content(bytes, expected_uid);
    }
    crate::artifacts::items_io::validate_content_uid(content, expected_uid)
}
