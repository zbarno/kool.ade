use super::*;
use std::os::unix::fs::PermissionsExt;

const PI_HELP_FLAGS: [&str; 19] = [
    "--print",
    "--mode",
    "--no-session",
    "--no-approve",
    "--append-system-prompt",
    "--thinking",
    "--provider",
    "--model",
    "--no-extensions",
    "--no-skills",
    "--no-prompt-templates",
    "--no-themes",
    "--no-context-files",
    "--no-tools",
    "--no-builtin-tools",
    "--tools",
    "--extension",
    "json",
    "xhigh",
];

pub(super) struct Fixture {
    pub(super) root: std::path::PathBuf,
}

impl Fixture {
    pub(super) fn new(auth_body: &str) -> (Self, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "koolade-pi-auth-assessment-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("invoked");
        let binary = root.join("pi");
        let help_tokens = PI_HELP_FLAGS
            .iter()
            .map(|token| format!("'{token}'"))
            .collect::<Vec<_>>()
            .join(" ");
        // Records the exact argv it received so tests can pin the bounded
        // invocation shape, then answers each probe command deterministically.
        std::fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s|%s|%s|%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" \"$4\" \"$5\" \"$6\" >> '{marker}'\ncase \"$1\" in\n--version) echo 'pi 1.2.3'; exit 0;;\n--help) printf '%s\\n' {help}; exit 0;;\nauth) {body};; esac\nexit 3\n",
                marker = marker.display(),
                help = help_tokens,
                body = auth_body,
            ),
        ).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        (Fixture { root }, binary)
    }

    pub(super) fn invoked_args(&self) -> Option<Vec<String>> {
        let log = std::fs::read_to_string(self.root.join("invoked")).ok()?;
        let first = log.lines().next()?;
        Some(
            first
                .trim_end_matches('|')
                .split('|')
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Args captured specifically for the auth invocation, if any.
    pub(super) fn invoked_auth_args(&self) -> Option<Vec<String>> {
        let log = std::fs::read_to_string(self.root.join("invoked")).ok()?;
        log.lines()
            .find(|line| line.starts_with("auth"))
            .map(|line| {
                line.trim_end_matches('|')
                    .split('|')
                    .map(str::to_owned)
                    .collect()
            })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn pi_settings_home(fixture: &Fixture) -> std::path::PathBuf {
    let home = fixture.root.join("home");
    std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
    std::fs::write(
        home.join(".pi/agent/settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"model-a"}"#,
    )
    .unwrap();
    std::fs::write(
        home.join(".pi/agent/models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"model-a"}]}}}"#,
    )
    .unwrap();
    home
}

/// Production boundary: discovery probe -> local Pi profile -> result conversion.
/// A fake CLI plus a temporary HOME are the only dependencies (bounded subprocess).
#[test]
fn ready_auth_check_verifies_the_configured_provider() {
    let (fixture, binary) = Fixture::new(
        r#"echo '{"status":"ready","provider":"synthetic","authType":"api_key"}'; exit 0"#,
    );
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Ready);
    assert!(assessment.live_provider_verified);
    assert!(!assessment.configuration_required);
    assert_eq!(assessment.diagnostic, None);
    assert_eq!(fixture.invoked_args(), Some(expected_argv()));
}

/// Production boundary: discovery probe -> bounded argv -> unrecognized-result
/// handling. Empty probe output must never masquerade as readiness.
#[test]
fn auth_check_invocation_is_bounded_and_empty_output_stays_unknown() {
    let (fixture, binary) = Fixture::new(r#"exit 0"#);
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(fixture.invoked_args(), Some(expected_argv()));
    assert_eq!(assessment.state, ReadinessState::Unknown);
}

fn expected_argv() -> Vec<String> {
    [
        "auth",
        "check",
        "--provider",
        "synthetic",
        "--json",
        "--no-refresh",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

/// Production boundary: the readiness probe must not execute user-defined host
/// code. `auth check` honors Pi's local credential profile exactly as a real
/// run does, so these tests pin the exact argv: no shell interpolation, no
/// refresh side effects, no spawning for out-of-class provider identifiers.
#[test]
fn provider_identifier_validation_blocks_out_of_class_values_without_spawning() {
    for provider in ["bad provider", "", "-leading", &"x".repeat(65)] {
        let (fixture, binary) = Fixture::new(r#"echo hi; exit 0"#);
        let assessment = evaluate_pi_authentication(Some(&binary), Some(provider));
        assert_eq!(assessment.state, ReadinessState::Unknown, "{provider:?}");
        assert!(!assessment.live_provider_verified);
        assert!(fixture.invoked_args().is_none(), "{provider:?}");
    }
    let (fixture, binary) = Fixture::new(r#"echo hi; exit 0"#);
    let assessment = evaluate_pi_authentication(Some(&binary), None);
    assert_eq!(assessment.state, ReadinessState::Unknown);
    assert!(fixture.invoked_args().is_none());
}
#[test]
fn missing_sign_in_maps_to_configuration_required_with_actionable_detail() {
    let (_fixture, binary) = Fixture::new(
        r#"echo '{"status":"not_ready","provider":"synthetic","reason":"credentials_not_configured"}'; exit 1"#,
    );
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Unavailable);
    assert!(assessment.configuration_required);
    assert!(!assessment.live_provider_verified);
    let diagnostic = assessment.diagnostic.expect("diagnostic required");
    assert!(diagnostic.contains("sign-in"), "{diagnostic}");
    assert!(
        diagnostic.contains("credentials_not_configured"),
        "{diagnostic}"
    );
}

#[test]
fn unknown_configured_provider_maps_to_configuration_error() {
    let (_fixture, binary) = Fixture::new(
        r#"echo '{"status":"not_ready","provider":"ghost","reason":"provider_not_found"}'; exit 1"#,
    );
    let assessment = evaluate_pi_authentication(Some(&binary), Some("ghost"));
    assert_eq!(assessment.state, ReadinessState::Unavailable);
    assert!(assessment.configuration_required);
    let diagnostic = assessment.diagnostic.expect("diagnostic required");
    assert!(diagnostic.contains("ghost"), "{diagnostic}");
    assert!(diagnostic.contains("recognize"), "{diagnostic}");
}

#[test]
fn unrecognized_or_failed_probe_results_stay_unknown_with_bounded_detail() {
    let (_fixture, binary) = Fixture::new("echo not-json-at-all; exit 1");
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Unknown);
    assert!(!assessment.configuration_required);
    assert!(assessment.diagnostic.unwrap().contains("not-json-at-all"));

    let (_fixture, binary) =
        Fixture::new(r#"echo '{"status":"surprising","provider":"synthetic"}'; exit 0"#);
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Unknown);
    assert!(assessment.diagnostic.unwrap().contains("unexpected status"));
}

/// End-to-end discovery: a live ready `auth check` flips the persisted Pi
/// record from permanently-unverified to verified (production boundary:
/// discovery worker output feeding the persisted readiness contract).
#[test]
fn pi_live_auth_check_verifies_provider_and_catalog_end_to_end() {
    let (fixture, binary) = Fixture::new(
        r#"echo '{"status":"ready","provider":"synthetic","authType":"api_key"}'; exit 0"#,
    );
    let home = pi_settings_home(&fixture);
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", home.as_os_str());
    environment.remove("PI_CODING_AGENT_DIR");
    environment.set(
        crate::harness::pi_harness::PI_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = super::super::discover_provider("pi", true);
    assert_eq!(report.authentication, ReadinessState::Ready);
    assert!(report.model_catalog_fresh);
    assert!(report.ready);
    assert!(!report.configuration_required);

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("pi"),
        1,
    );
    let detected = &settings.discovered["pi"];
    assert!(detected.ready);
    assert!(detected.implementation_available);
    assert_eq!(
        detected.model_catalog_status(),
        crate::harness::discovery::ModelCatalogStatus::Fresh
    );
    let now = crate::persistence::harness_settings::current_unix_ms();
    assert!(super::super::super::super::has_current_provider_capabilities("pi", detected, now));
    assert!(detected.has_current_model_catalog_at("pi", now));
    assert!(detected.has_any_verified_model_at("pi", now));
    assert_eq!(
        detected.verified_default_model_at("pi", now),
        Some("model-a")
    );
    let snapshot = detected.capability_snapshot.as_ref().unwrap();
    assert_eq!(snapshot.authentication, ReadinessState::Ready);
    assert_eq!(snapshot.model_catalog, ReadinessState::Ready);
}

/// Counterpart: local settings alone still never verify anything. A not_ready
/// account keeps Pi sign-in-pending, catalog unfresh, and routing ineligible.
#[test]
fn pi_unauthenticated_account_keeps_routing_closed_end_to_end() {
    let (fixture, binary) = Fixture::new(
        r#"echo '{"status":"not_ready","provider":"synthetic","reason":"credentials_not_configured"}'; exit 1"#,
    );
    let home = pi_settings_home(&fixture);
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", home.as_os_str());
    environment.remove("PI_CODING_AGENT_DIR");
    environment.set(
        crate::harness::pi_harness::PI_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = super::super::discover_provider("pi", true);
    assert_eq!(report.authentication, ReadinessState::Unavailable);
    assert!(report.configuration_required);
    assert!(!report.model_catalog_fresh);

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("pi"),
        1,
    );
    let detected = &settings.discovered["pi"];
    assert!(!detected.ready);
    assert!(detected.configuration_required);
    assert!(
        detected
            .diagnostic
            .as_deref()
            .is_some_and(|detail| detail.contains("sign-in"))
    );
    let now = crate::persistence::harness_settings::current_unix_ms();
    assert!(!super::super::super::super::has_current_provider_capabilities("pi", detected, now));
    assert!(!detected.has_current_model_catalog_at("pi", now));
}
