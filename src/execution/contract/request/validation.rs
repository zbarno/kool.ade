use std::collections::BTreeSet;

pub(super) fn validate_working_directory(value: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(value);
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 1024
            && !value.contains('\\')
            && !value.contains('\0')
            && path.components().all(|component| matches!(
                component,
                std::path::Component::Normal(_)
            ) || component == std::path::Component::CurDir),
        "execution working directory must remain inside the task workspace"
    );
    Ok(())
}

pub(super) fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(super) fn validate_environment_names(names: &BTreeSet<String>) -> anyhow::Result<()> {
    for name in names {
        anyhow::ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                && !name.as_bytes()[0].is_ascii_digit(),
            "environment allowlist contains an invalid variable name"
        );
        anyhow::ensure!(
            !crate::project_runtime::is_protected_environment_key(name),
            "credential environment variables cannot be granted to an execution"
        );
    }
    Ok(())
}
