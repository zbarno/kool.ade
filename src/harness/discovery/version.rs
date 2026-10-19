/// Extract only a numeric version from a recognized provider version line.
/// Arbitrary CLI stdout must never be persisted or rendered as version metadata.
pub(crate) fn normalize_cli_version(
    output: &str,
    identities: &[&str],
    allow_bare_version: bool,
) -> Option<String> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let remainder = identities.iter().find_map(|identity| {
            let prefix = line.get(..identity.len())?;
            if !prefix.eq_ignore_ascii_case(identity) {
                return None;
            }
            let remainder = line.get(identity.len()..)?;
            let boundary = remainder.chars().next()?;
            matches!(boundary, ' ' | '\t' | ':' | '-' | 'v' | 'V').then_some(remainder)
        });
        let remainder = remainder.or_else(|| {
            (allow_bare_version && line.chars().next()?.is_ascii_digit()).then_some(line)
        })?;
        numeric_version(remainder)
    })
}

fn numeric_version(remainder: &str) -> Option<String> {
    remainder.split_whitespace().find_map(|candidate| {
        let candidate = candidate
            .trim_start_matches([' ', '\t', ':', '-', 'v', 'V'])
            .trim_end_matches('.');
        let mut parts = candidate.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch = parts.next()?.parse::<u64>().ok()?;
        parts
            .next()
            .is_none()
            .then(|| format!("{major}.{minor}.{patch}"))
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_cli_version;

    #[test]
    fn provider_version_lines_are_normalized_without_retaining_extra_output() {
        for (output, identity, bare) in [
            ("codex-cli 1.2.3 token=synthetic-secret", "codex-cli", false),
            ("agy 1.2.3 token=synthetic-secret", "agy", false),
            (
                "Claude Code v1.2.3 token=synthetic-secret",
                "Claude Code",
                true,
            ),
            (
                "GitHub Copilot CLI 1.2.3. token=synthetic-secret",
                "GitHub Copilot CLI",
                false,
            ),
            ("opencode 1.2.3 token=synthetic-secret", "opencode", true),
            ("pi - v1.2.3 token=synthetic-secret", "pi", true),
        ] {
            let version = normalize_cli_version(output, &[identity], bare).unwrap();
            assert_eq!(version, "1.2.3");
            assert!(!version.contains("synthetic-secret"));
        }
    }

    #[test]
    fn arbitrary_preambles_and_malformed_versions_are_rejected() {
        assert_eq!(
            normalize_cli_version("token=synthetic-secret 1.2.3", &["codex-cli"], false),
            None
        );
        assert_eq!(normalize_cli_version("agy secret", &["agy"], false), None);
        assert_eq!(
            normalize_cli_version("opencode 1.2.secret", &["opencode"], true),
            None
        );
    }
}
