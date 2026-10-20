use std::{path::Path, time::Duration};

const GITHUB_IDENTITY_TIMEOUT: Duration = Duration::from_secs(15);

/// Resolve the authenticated GitHub account used to authorize a self-update.
/// The GitHub CLI reads its saved host login; the probe environment excludes
/// token environment variables and all shell/provider startup hooks.
pub(crate) fn authenticated_github_user_id(gh_bin: &Path) -> Result<String, String> {
    let argv = [
        gh_bin.to_string_lossy().into_owned(),
        "api".into(),
        "user".into(),
        "--jq".into(),
        ".id".into(),
        "--hostname".into(),
        "github.com".into(),
    ];
    let output = crate::harness::discovery::run(&argv, GITHUB_IDENTITY_TIMEOUT, 64, 1024, &[])
        .map_err(|error| format!("could not verify the authenticated GitHub account: {error}"))?;
    if !output.success {
        return Err("GitHub authentication is required to confirm this installation".into());
    }
    let user_id = std::str::from_utf8(&output.stdout)
        .map_err(|_| "GitHub returned an invalid authenticated account identity".to_owned())?
        .trim();
    if !valid_github_user_id(user_id) {
        return Err("GitHub returned an invalid authenticated account identity".into());
    }
    Ok(user_id.to_owned())
}

pub(super) fn valid_github_user_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte != b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_account_ids_are_positive_bounded_decimal_values() {
        assert!(valid_github_user_id("5166344"));
        assert!(!valid_github_user_id(""));
        assert!(!valid_github_user_id("0"));
        assert!(!valid_github_user_id("12x"));
        assert!(!valid_github_user_id("123456789012345678901"));
    }

    /// Production boundary: the installation approval identity comes from a
    /// bounded app-owned CLI probe, with inherited token variables stripped.
    /// Unit validation cannot prove subprocess arguments and environment
    /// sanitization. Uses a temporary executable only; no network or Docker.
    /// Setup cost is one small script and a temporary directory.
    #[cfg(unix)]
    #[test]
    fn authenticated_account_probe_uses_github_profile_without_token_environment() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("gh");
        std::fs::write(
            &executable,
            "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\n[ -z \"${GH_TOKEN+x}\" ] || exit 3\nprintf '5166344\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(
            authenticated_github_user_id(&executable).unwrap(),
            "5166344"
        );
    }
}
