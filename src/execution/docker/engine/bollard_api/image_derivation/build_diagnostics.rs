/// Build output can contain arbitrary package-script text. Keep diagnostics short,
/// single-line, and redact credential-bearing fields before returning them to callers.
pub(super) fn sanitize_build_diagnostic(diagnostic: &str) -> String {
    let normalized = diagnostic
        .chars()
        .take(2_048)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let redacted_urls = redact_url_userinfo(&normalized);
    let redacted_fields = redact_assignments(&redacted_urls);
    let compact = redacted_fields
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    compact.chars().take(320).collect()
}

fn redact_url_userinfo(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(scheme_end) = remaining.find("://") {
        let authority_start = scheme_end + 3;
        let authority_end = remaining[authority_start..]
            .find(['/', '?', '#', ' ', '&', ','])
            .map_or(remaining.len(), |offset| authority_start + offset);
        let authority = &remaining[authority_start..authority_end];
        let Some(at) = authority.find('@') else {
            result.push_str(&remaining[..authority_start]);
            remaining = &remaining[authority_start..];
            continue;
        };
        result.push_str(&remaining[..authority_start]);
        result.push_str("[redacted]@");
        result.push_str(&authority[at + 1..]);
        remaining = &remaining[authority_end..];
    }
    result.push_str(remaining);
    result
}

fn redact_assignments(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut redact_following_value = false;
    for word in value.split_whitespace() {
        if redact_following_value {
            if !word.starts_with('-') {
                redact_following_value = false;
                if word.eq_ignore_ascii_case("bearer") {
                    result.push_str("[redacted] [redacted] ");
                    redact_following_value = true;
                } else {
                    result.push_str("[redacted] ");
                }
                continue;
            }
            // An option flag cannot be the value for the preceding secret flag.
            // Process it normally so a second secret flag still masks its value.
            redact_following_value = false;
        }
        if word.eq_ignore_ascii_case("bearer") {
            result.push_str("Bearer [redacted] ");
            redact_following_value = true;
            continue;
        }
        result.push_str(&redact_word_assignments(word, &mut redact_following_value));
        result.push(' ');
    }
    result.trim_end().to_owned()
}

fn redact_word_assignments(word: &str, redact_following_value: &mut bool) -> String {
    let mut result = String::with_capacity(word.len());
    for (index, part) in word.split_inclusive(['&', ',', ';']).enumerate() {
        let assignment = part.trim_end_matches(['&', ',', ';']);
        let suffix = &part[assignment.len()..];
        let separator = assignment.find('=').or_else(|| assignment.find(':'));
        let Some(separator) = separator else {
            let flag = assignment.trim_start_matches('-');
            if assignment.starts_with('-') && is_sensitive_key(flag) {
                result.push_str(part);
                *redact_following_value = true;
            } else {
                result.push_str(part);
            }
            continue;
        };
        let raw_key = &assignment[..separator];
        let key = raw_key
            .rsplit(['?', '/', '#'])
            .next()
            .unwrap_or(raw_key)
            .trim_matches(|character: char| !character.is_ascii_alphanumeric());
        if !is_sensitive_key(key) {
            result.push_str(part);
            continue;
        }
        result.push_str(raw_key);
        result.push_str(&assignment[separator..=separator]);
        result.push_str("[redacted]");
        result.push_str(suffix);
        if is_authorization_key(key) && index == 0 {
            *redact_following_value = true;
        }
    }
    result
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    let components = normalized.split('_').collect::<Vec<_>>();
    components.iter().any(|component| {
        matches!(
            *component,
            "token"
                | "secret"
                | "password"
                | "passwd"
                | "credential"
                | "credentials"
                | "authorization"
                | "auth"
                | "signature"
        )
    }) || normalized == "key"
        || [
            "api_key",
            "access_key",
            "private_key",
            "signing_key",
            "ssh_key",
        ]
        .iter()
        .any(|suffix| normalized.contains(suffix))
}

fn is_authorization_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().replace('-', "_").as_str(),
        "authorization" | "proxy_authorization"
    )
}

#[cfg(test)]
mod tests {
    use super::sanitize_build_diagnostic;

    #[test]
    fn build_diagnostics_redact_url_query_bearer_and_assignment_secrets() {
        let diagnostic = concat!(
            "failed https://build-user:url-pass@example.invalid/path?token=query-secret&",
            "X-Amz-Signature=signed-secret; keep=visible ",
            "Bearer bearer-secret API_KEY=api-secret password:pass-secret ",
            "Authorization: Bearer header-secret --password cli-pass --token cli-token ",
            "--password --token consecutive-flag-secret AWS_SECRET_ACCESS_KEY=aws-secret"
        );
        let sanitized = sanitize_build_diagnostic(diagnostic);
        for secret in [
            "url-pass",
            "query-secret",
            "signed-secret",
            "bearer-secret",
            "api-secret",
            "pass-secret",
            "header-secret",
            "cli-pass",
            "cli-token",
            "consecutive-flag-secret",
            "aws-secret",
        ] {
            assert!(!sanitized.contains(secret), "leaked {secret}: {sanitized}");
        }
        assert!(sanitized.contains("keep=visible"));
        assert!(sanitized.contains("https://[redacted]@example.invalid"));
    }

    #[test]
    fn build_diagnostics_are_single_line_and_bounded() {
        let sanitized = sanitize_build_diagnostic(&format!("first\n{}", "x".repeat(4_000)));
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.len() <= 320);
    }
}
