use super::sanitized_reason;

#[test]
fn sensitive_markers_redact_whitespace_separated_credential_values() {
    for (diagnostic, secret) in [
        ("Authorization: Bearer abc123", "abc123"),
        ("authorization=Bearer abc123", "abc123"),
        ("token= abc123", "abc123"),
        ("password= abc123", "abc123"),
        ("API key: spaced-secret", "spaced-secret"),
        ("access_token = spaced-token-secret", "spaced-token-secret"),
        ("API key = spaced-api-secret", "spaced-api-secret"),
        ("{\"accessToken\":\"json-secret\"}", "json-secret"),
        (
            "{\"message\":\"failed\",\"accessToken\":\"later-json-secret\"}",
            "later-json-secret",
        ),
    ] {
        let sanitized = sanitized_reason(diagnostic);
        assert!(sanitized.contains("[redacted]"), "{diagnostic}");
        assert!(!sanitized.contains(secret), "{diagnostic}: {sanitized}");
    }
}

#[test]
fn sensitive_argument_names_redact_unmarked_values() {
    for (key, value) in [
        ("password", "password-secret"),
        ("accessToken", "access-token-secret"),
        ("API key", "api-key-secret"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret-access-key"),
        ("secret_value", "secret-value"),
        ("private_key_pem", "private-key-material"),
        ("AWS_ACCESS_KEY_ID", "aws-access-key-id"),
        ("bearer_token", "bearer-token-secret"),
    ] {
        let (_, sanitized) = crate::runtime_qa::diagnostic_redaction::sanitized_field(key, value);
        assert_eq!(sanitized, "[redacted]", "{key}");
        assert!(!sanitized.contains(value));
    }
}

#[test]
fn compound_credential_labels_redact_values_in_driver_diagnostics() {
    for (diagnostic, secret) in [
        ("AWS_SECRET_ACCESS_KEY=aws-secret", "aws-secret"),
        ("secret_value: labeled-secret", "labeled-secret"),
        ("private_key_pem=private-material", "private-material"),
        ("AWS_ACCESS_KEY_ID=access-identifier", "access-identifier"),
        ("bearer_token=bearer-secret", "bearer-secret"),
    ] {
        let sanitized = sanitized_reason(diagnostic);
        assert!(sanitized.contains("[redacted]"), "{diagnostic}");
        assert!(!sanitized.contains(secret), "{diagnostic}: {sanitized}");
    }
}
