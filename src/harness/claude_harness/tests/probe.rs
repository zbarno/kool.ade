use super::super::{ClaudeHarness, ClaudeReadiness, auth_method_readiness, parse_version, probe};
#[cfg(unix)]
use super::support::{fake_cli, temp_dir};

#[test]
fn version_parser_enforces_supported_version_shape() {
    assert_eq!(parse_version("2.1.268"), Some((2, 1, 268)));
    assert_eq!(parse_version("Claude Code v2.1.300"), Some((2, 1, 300)));
    assert_eq!(parse_version("unknown"), None);
}

#[test]
fn auth_method_parser_keeps_unknown_access_unverified() {
    for method in [
        "claude.ai",
        "oauth_token",
        "api_key",
        "api_key_helper",
        "third_party",
    ] {
        assert_eq!(auth_method_readiness(method), ClaudeReadiness::Ready);
    }
    assert_eq!(
        auth_method_readiness("none"),
        ClaudeReadiness::AuthenticationRequired
    );
    assert_eq!(
        auth_method_readiness("future_method"),
        ClaudeReadiness::AuthenticationUnverified
    );
}

#[cfg(unix)]
#[test]
fn probe_distinguishes_ready_unsupported_and_missing_auth() {
    let root = temp_dir("probe");
    let binary = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"claude.ai\"}'; fi\nexit 0",
    );
    assert_eq!(probe(&binary).unwrap(), "2.1.300");
    let old = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.100'; else echo '{}'; fi\nexit 0",
    );
    assert_eq!(probe(&old).unwrap_err().0, ClaudeReadiness::Unsupported);
    let unauth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"none\"}'; fi\nexit 0",
    );
    assert_eq!(
        probe(&unauth).unwrap_err().0,
        ClaudeReadiness::AuthenticationRequired
    );
    let broken_auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else exit 2; fi\n",
    );
    let failure = probe(&broken_auth).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::AuthenticationUnverified);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    let unauth_nonzero = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"none\"}'; exit 1; fi\n",
    );
    let failure = probe(&unauth_nonzero).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::AuthenticationRequired);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    let unknown_auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"mystery\",\"token\":\"synthetic-secret\"}'; fi\nexit 0",
    );
    let failure = probe(&unknown_auth).unwrap_err();
    assert_eq!(failure.0, ClaudeReadiness::AuthenticationUnverified);
    assert_eq!(failure.2.as_deref(), Some("2.1.300"));
    assert!(!failure.1.contains("synthetic-secret"));
    assert!(
        ClaudeHarness::checked_binary(&unknown_auth)
            .unwrap_err()
            .detail()
            .contains("authentication status is unrecognized")
    );

    let oauth_auth = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"oauth_token\"}'; fi\nexit 0",
    );
    assert_eq!(probe(&oauth_auth).unwrap(), "2.1.300");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn readiness_probe_overrides_a_hostile_parent_update_setting() {
    let root = temp_dir("update-lock");
    let binary = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo '2.1.300'; else echo '{\"authMethod\":\"claude.ai\"}'; fi\nexit 0",
    );
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("DISABLE_UPDATES", "0");

    assert_eq!(probe(&binary).unwrap(), "2.1.300");
    drop(environment);
    std::fs::remove_dir_all(root).unwrap();
}
