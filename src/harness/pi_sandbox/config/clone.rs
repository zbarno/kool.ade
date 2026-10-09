use std::path::Path;

pub(in crate::harness::pi_sandbox) fn validate_koolade_clone(
    root: &Path,
    common: &Path,
) -> anyhow::Result<()> {
    let projects = crate::persistence::state_root()
        .join("projects")
        .canonicalize()?;
    anyhow::ensure!(
        root.starts_with(&projects),
        "Task clone is outside Kool.ad/e's private project storage"
    );
    let relative = root.strip_prefix(&projects)?;
    let parts = relative.components().collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() == 4
            && parts[1].as_os_str() == "task-repositories"
            && valid_repository_component(parts[2].as_os_str())
            && valid_task_repository_component(parts[3].as_os_str())
            && parts[0].as_os_str() != ""
            && common == root.join(".git"),
        "Task clone path does not match Kool.ad/e's allocation"
    );
    Ok(())
}

fn valid_repository_component(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_task_repository_component(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let valid_key = |base: &str| {
        base.rsplit_once('-').is_some_and(|(slug, hash)| {
            !slug.is_empty()
                && hash.len() == 16
                && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    for marker in ["-integration-", "-verification-"] {
        if let Some((base, suffix)) = name.rsplit_once(marker)
            && suffix.len() == 12
            && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return valid_key(base);
        }
    }
    valid_key(name)
}
