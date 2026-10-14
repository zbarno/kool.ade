use super::{MAX_WORKSPACE_DEPTH, WorkspaceEntryV1};

pub(in crate::execution::remote::workspace) fn root_digest(
    entries: &[WorkspaceEntryV1],
    excluded_paths: &[String],
) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let encoded = serde_json::to_vec(&(entries, excluded_paths))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub(in crate::execution::remote::workspace) fn validate_relative_path(
    path: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains(':')
            && !path.chars().any(char::is_control)
            && path.len() <= 4096
            && path.split('/').count() <= MAX_WORKSPACE_DEPTH
            && path.split('/').all(valid_component),
        "workspace path is not a normalized relative path"
    );
    Ok(())
}

fn valid_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with('.')
        || component.ends_with(' ')
        || component
            .chars()
            .any(|character| "*?\"<>|".contains(character))
    {
        return false;
    }
    let basename = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(basename.len() == 4
            && (basename.starts_with("COM") || basename.starts_with("LPT"))
            && matches!(basename.as_bytes()[3], b'1'..=b'9'))
}

pub(in crate::execution::remote::workspace) fn valid_symlink_target(
    path: &str,
    target: &str,
) -> bool {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.contains(':')
        || target
            .chars()
            .any(|character| "*?\"<>|".contains(character))
        || target.chars().any(char::is_control)
    {
        return false;
    }
    let mut depth = path.split('/').count().saturating_sub(1);
    for component in target.split('/') {
        match component {
            "" | "." => return false,
            ".." if depth == 0 => return false,
            ".." => depth -= 1,
            _ if valid_component(component) => depth += 1,
            _ => return false,
        }
    }
    true
}

pub(in crate::execution::remote::workspace) fn is_excluded_component(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == ".git"
        || name == ".env"
        || name.starts_with(".env.")
        || matches!(
            name.as_str(),
            ".aws"
                | ".azure"
                | ".docker"
                | ".git-credentials"
                | ".gnupg"
                | ".kube"
                | ".netrc"
                | ".npmrc"
                | ".pypirc"
                | ".ssh"
                | ".terraform"
                | "auth.json"
                | "authorized_keys"
                | "credentials"
                | "credentials.json"
                | "credentials.toml"
                | "secrets"
                | "token"
                | "token.json"
        )
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
}

pub(in crate::execution::remote::workspace) fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{valid_symlink_target, validate_relative_path};

    #[test]
    fn rejects_non_normalized_and_absolute_manifest_paths() {
        for path in [
            "",
            "/etc/passwd",
            "../escape",
            "a/../b",
            "C:/secret",
            "a\\b",
        ] {
            assert!(validate_relative_path(path).is_err(), "accepted {path:?}");
        }
        validate_relative_path("src/main.rs").unwrap();
    }

    #[test]
    fn symlink_targets_may_walk_inside_but_never_above_the_workspace() {
        assert!(valid_symlink_target("src/nested/link", "../../Cargo.toml"));
        assert!(!valid_symlink_target("link", "../../outside"));
        assert!(!valid_symlink_target("link", "/etc/passwd"));
    }
}
