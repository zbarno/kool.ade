fn validate_source_paths(paths: &[String], kind: &str) -> anyhow::Result<()> {
    anyhow::ensure!(paths.len() <= 128, "too many approved source {kind} paths");
    let mut unique = std::collections::BTreeSet::new();
    for value in paths {
        let path = std::path::Path::new(value);
        anyhow::ensure!(
            !value.is_empty()
                && value.len() <= 1024
                && !value.contains('\\')
                && !value.chars().any(char::is_control)
                && path.components().all(|component| matches!(
                    component,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                ))
                && !path
                    .components()
                    .any(|component| component.as_os_str() == ".git"),
            "approved source {kind} paths must remain inside the workspace"
        );
        anyhow::ensure!(
            unique.insert(value),
            "approved source {kind} paths must be unique"
        );
    }
    Ok(())
}

pub(super) fn validate_source_input_paths(paths: &[String]) -> anyhow::Result<()> {
    validate_source_paths(paths, "input")
}

pub(super) fn validate_source_output_paths(paths: &[String]) -> anyhow::Result<()> {
    validate_source_paths(paths, "output")
}
