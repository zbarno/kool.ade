use sha2::Digest as _;

pub(super) fn hash_task_spec(approved_plan_sha: &str, task_text: &str) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(1_u16, approved_plan_sha, task_text))
        .map_err(|error| format!("task specification cannot be encoded: {error}"))?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}

pub(in crate::app::root::tick::activity::runtime_qa) fn acceptance_criteria(
    text: &str,
) -> Result<Vec<String>, String> {
    let mut in_section = false;
    let mut values = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            in_section = trimmed
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("acceptance criteria");
            continue;
        }
        if in_section {
            let value = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| trimmed.strip_prefix("+ "));
            if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
                values.push(value.to_owned());
            } else if !trimmed.is_empty() {
                return Err("task acceptance criteria must be explicit Markdown list items".into());
            }
        }
    }
    if values.is_empty() || values.len() > 512 {
        return Err(
            "the immutable task specification has no bounded acceptance-criteria list".into(),
        );
    }
    Ok(values)
}
