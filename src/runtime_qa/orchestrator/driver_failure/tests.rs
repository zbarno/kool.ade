use super::sanitized_reason;

#[test]
fn sensitive_markers_redact_whitespace_separated_credential_values() {
    for diagnostic in [
        "Authorization: Bearer abc123",
        "authorization=Bearer abc123",
        "token= abc123",
        "password= abc123",
    ] {
        let sanitized = sanitized_reason(diagnostic);
        assert!(sanitized.contains("[redacted]"), "{diagnostic}");
        assert!(!sanitized.contains("abc123"), "{diagnostic}: {sanitized}");
    }
}
