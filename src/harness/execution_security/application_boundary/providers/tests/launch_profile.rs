use super::super::{CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch};
use std::path::Path;

fn complete_evidence(provider: CliProvider, version: &str) -> ProfileProbeEvidence {
    ProfileProbeEvidence::validated(
        provider,
        Path::new("/bin/true"),
        version.to_owned(),
        provider.profile_requirements().capabilities,
    )
    .unwrap()
}

#[test]
fn evidence_enforces_discoverable_version_and_capability_requirements() {
    let binary = Path::new("/bin/true");
    assert!(
        ProfileProbeEvidence::validated(
            CliProvider::Claude,
            binary,
            "2.1.267".into(),
            CliProvider::Claude.profile_requirements().capabilities,
        )
        .is_err()
    );
    assert!(
        ProfileProbeEvidence::validated(
            CliProvider::Claude,
            binary,
            "2.1.268".into(),
            CliProvider::Claude.profile_requirements().capabilities,
        )
        .is_ok()
    );
    assert!(
        ProfileProbeEvidence::validated(
            CliProvider::Codex,
            binary,
            "codex 1.0".into(),
            &["--json"],
        )
        .is_err()
    );
}

#[test]
fn missing_profile_capability_has_a_typed_failure_for_manager_evidence() {
    let error = ProfileProbeEvidence::validated(
        CliProvider::Codex,
        Path::new("/bin/true"),
        "codex 1.0.0".into(),
        &["--json"],
    )
    .unwrap_err();
    let cause = error
        .downcast_ref::<crate::error::ProviderCapabilityMismatchCause>()
        .expect("missing profile capability remains typed");

    assert_eq!(cause.provider, "Codex");
    assert!(
        cause
            .missing
            .iter()
            .any(|capability| capability == "--model")
    );
}

#[test]
fn every_provider_launch_profile_binds_its_required_arguments_and_environment() {
    for provider in [
        CliProvider::Codex,
        CliProvider::Claude,
        CliProvider::Antigravity,
        CliProvider::OpenCode,
        CliProvider::Copilot,
        CliProvider::Pi,
    ] {
        let requirements = provider.profile_requirements();
        let evidence = ProfileProbeEvidence::validated(
            provider,
            Path::new("/bin/true"),
            "version 99.0.0".into(),
            requirements.capabilities,
        )
        .unwrap();
        let mut argv = vec!["/bin/true".to_owned()];
        argv.extend(requirements.launch_tokens().iter().map(|token| {
            if token.ends_with('=') {
                format!("{token}read")
            } else {
                (*token).to_owned()
            }
        }));
        let env = requirements
            .environment_keys()
            .iter()
            .map(|key| ((*key).to_owned(), "/tmp/profile".to_owned()))
            .collect();
        assert!(
            ValidatedProviderLaunch::bind(evidence, argv, Path::new("/tmp"), env).is_ok(),
            "{provider:?}"
        );
    }
}

#[test]
fn profile_drift_fails_before_process_creation() {
    let provider = CliProvider::Claude;
    let evidence = complete_evidence(provider, "2.1.268");
    let mut launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--effort".into(),
            "high".into(),
        ],
        Path::new("/tmp"),
        vec![],
    )
    .unwrap();
    launch.add_test_argv_drift();
    assert!(launch.spawn_with_input(Path::new("/tmp"), None).is_err());

    let evidence = complete_evidence(provider, "2.1.268");
    let mut launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--effort".into(),
            "high".into(),
        ],
        Path::new("/tmp"),
        vec![],
    )
    .unwrap();
    launch.add_test_environment_drift();
    assert!(launch.spawn_with_input(Path::new("/tmp"), None).is_err());
}

#[test]
fn launch_rejects_working_directory_drift() {
    let provider = CliProvider::Claude;
    let evidence = complete_evidence(provider, "2.1.268");
    let launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--effort".into(),
            "high".into(),
        ],
        Path::new("/tmp"),
        vec![],
    )
    .unwrap();

    assert!(launch.spawn_with_input(Path::new("/"), None).is_err());
}

#[test]
fn launch_binding_rejects_a_different_executable_and_missing_profile_parts() {
    let provider = CliProvider::Antigravity;
    let evidence = complete_evidence(provider, "1.0.0");
    assert!(
        ValidatedProviderLaunch::bind(
            evidence.clone(),
            vec!["/bin/false".into()],
            Path::new("/tmp"),
            vec![],
        )
        .is_err()
    );
    assert!(
        ValidatedProviderLaunch::bind(
            evidence,
            vec!["/bin/true".into()],
            Path::new("/tmp"),
            vec![
                ("HOME".into(), "/tmp".into()),
                ("XDG_CONFIG_HOME".into(), "/tmp".into())
            ],
        )
        .is_err()
    );
    let evidence = complete_evidence(provider, "1.0.0");
    assert!(
        ValidatedProviderLaunch::bind(
            evidence,
            vec![
                "/bin/true".into(),
                "--input-format".into(),
                "stream-json".into(),
                "--output-format".into(),
                "stream-json".into(),
                "--sandbox".into(),
            ],
            Path::new("/tmp"),
            vec![
                ("HOME".into(), String::new()),
                ("XDG_CONFIG_HOME".into(), "/tmp/config".into()),
            ],
        )
        .is_err()
    );
}

#[test]
fn launch_telemetry_records_only_redacted_profile_identity() {
    let provider = CliProvider::Claude;
    let launch = ValidatedProviderLaunch::bind(
        complete_evidence(provider, "2.1.268"),
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--effort".into(),
            "high".into(),
        ],
        Path::new("/tmp"),
        vec![("ANTHROPIC_API_KEY".into(), "synthetic-secret".into())],
    )
    .unwrap();

    let telemetry = launch.telemetry(Some("implementer/v1"));
    assert_eq!(telemetry.provider, "Claude Code");
    assert_eq!(telemetry.cli_version, "2.1.268");
    assert_eq!(
        telemetry.prompt_role_version.as_deref(),
        Some("implementer/v1")
    );
    assert_eq!(
        telemetry.verified_capabilities.len(),
        provider.profile_requirements().capabilities.len()
    );
    assert_eq!(telemetry.profile_hash.len(), 64);
    let serialized = serde_json::to_string(&telemetry).unwrap();
    assert!(!serialized.contains("synthetic-secret"));
    assert!(!serialized.contains("ANTHROPIC_API_KEY"));
    assert!(!serialized.contains("/tmp"));
}
