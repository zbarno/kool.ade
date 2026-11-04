use super::{AuthenticationModeV1, authentication_mode_for_sources};

#[test]
fn codex_workload_identity_sources_are_not_misclassified_as_profile_auth() {
    for source in ["OPENAI_FEDERATION_RULE_ID", "OPENAI_IDENTITY_TOKEN_FILE"] {
        assert_eq!(
            authentication_mode_for_sources("codex", |name| name == source),
            AuthenticationModeV1::Environment,
            "{source} must invalidate profile-auth readiness"
        );
    }
    assert_eq!(
        authentication_mode_for_sources("codex", |_| false),
        AuthenticationModeV1::ProviderProfile
    );
}
