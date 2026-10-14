use super::super::{CliProvider, ProfileProbeEvidence, ValidatedProviderLaunch};
use std::path::Path;

#[test]
fn evidence_enforces_discoverable_version_and_capability_requirements() {
    let binary = Path::new("/bin/true");
    assert!(
        ProfileProbeEvidence::validated(CliProvider::Claude, binary, "2.1.267".into(), &[],)
            .is_err()
    );
    assert!(
        ProfileProbeEvidence::validated(CliProvider::Claude, binary, "2.1.268".into(), &[],)
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
    let evidence =
        ProfileProbeEvidence::validated(provider, Path::new("/bin/true"), "2.1.268".into(), &[])
            .unwrap();
    let mut launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
        ],
        Path::new("/tmp"),
        vec![],
    )
    .unwrap();
    launch.add_test_argv_drift();
    assert!(launch.spawn_with_input(Path::new("/tmp"), None).is_err());

    let evidence =
        ProfileProbeEvidence::validated(provider, Path::new("/bin/true"), "2.1.268".into(), &[])
            .unwrap();
    let mut launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
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
    let evidence =
        ProfileProbeEvidence::validated(provider, Path::new("/bin/true"), "2.1.268".into(), &[])
            .unwrap();
    let launch = ValidatedProviderLaunch::bind(
        evidence,
        vec![
            "/bin/true".into(),
            "--tools".into(),
            "--restricted".into(),
            "--strict-mcp-config".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
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
    let evidence =
        ProfileProbeEvidence::validated(provider, Path::new("/bin/true"), "1.0.0".into(), &[])
            .unwrap();
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
    let evidence =
        ProfileProbeEvidence::validated(provider, Path::new("/bin/true"), "1.0.0".into(), &[])
            .unwrap();
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
