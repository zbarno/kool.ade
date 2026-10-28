use super::tests::Fixture;
use super::*;

/// Crossed wires: a ready payload on a failing exit is an ambiguous outcome
/// and must fail closed, never masquerading as readiness.
#[test]
fn crossed_wire_ready_payload_on_failure_exit_fails_closed() {
    let (_fixture, binary) =
        Fixture::new(r#"echo '{"status":"ready","provider":"synthetic"}'; exit 1"#);
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Unknown);
    assert!(!assessment.live_provider_verified);
    assert!(
        assessment
            .diagnostic
            .as_deref()
            .is_some_and(|detail| detail.contains("unsuccessfully"))
    );
}

#[test]
fn spawn_failure_classifies_as_unknown_with_bounded_detail() {
    let assessment = evaluate_pi_authentication(
        Some(std::path::Path::new(
            "/definitely/absent/pi-binary-for-probe",
        )),
        Some("synthetic"),
    );
    assert_eq!(assessment.state, ReadinessState::Unknown);
    assert!(!assessment.configuration_required);
    assert!(
        assessment
            .diagnostic
            .as_deref()
            .is_some_and(|detail| detail.contains("could not be completed"))
    );
}

#[test]
fn blank_lines_before_the_json_payload_are_ignored() {
    let (_fixture, binary) =
        Fixture::new("echo ''; echo '{\"status\":\"ready\",\"provider\":\"synthetic\"}'; exit 0");
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Ready);
    assert!(assessment.live_provider_verified);
}

#[test]
fn credential_shaped_url_authorities_in_probe_details_are_screened() {
    let (_fixture, binary) = Fixture::new(
        r#"echo '{"status":"not_ready","provider":"synthetic","reason":"expiry at https://rotated-TOKEN-XYZ123@vault.local/v"}'; exit 1"#,
    );
    let assessment = evaluate_pi_authentication(Some(&binary), Some("synthetic"));
    assert_eq!(assessment.state, ReadinessState::Unavailable);
    let diagnostic = assessment.diagnostic.expect("diagnostic required");
    assert!(!diagnostic.contains("rotated-TOKEN-XYZ123"), "{diagnostic}");
    assert!(diagnostic.contains("@vault.local"), "{diagnostic}");
}

/// Positive counterpart to the out-of-class rejection: remote-style provider
/// identifiers carrying `:` and `/` must still reach the binary verbatim.
#[test]
fn remote_style_provider_identifiers_reach_the_binary_verbatim() {
    let (fixture, binary) =
        Fixture::new(r#"echo '{"status":"ready","provider":"remote-org/team:beta"}'; exit 0"#);
    let assessment = evaluate_pi_authentication(Some(&binary), Some(" remote-org/team:beta "));
    assert_eq!(assessment.state, ReadinessState::Ready);
    assert!(assessment.live_provider_verified);
    assert_eq!(
        fixture.invoked_auth_args(),
        Some(vec![
            "auth".to_owned(),
            "check".to_owned(),
            "--provider".to_owned(),
            "remote-org/team:beta".to_owned(),
            "--json".to_owned(),
            "--no-refresh".to_owned(),
        ])
    );
}

/// Production boundary (profile identity): when the operator points
/// `PI_CODING_AGENT_DIR` at a non-default agent directory, the readiness probe
/// must inspect THAT profile, not $HOME/.pi/agent — the fake answers from the
/// directory its own environment names, so a regression that drops the
/// override visibly demotes a signed-in profile to sign-in-pending.

#[test]
fn pi_probe_inspects_the_agent_directory_named_by_the_operators_environment() {
    let (fixture, binary) = Fixture::new(
        r#"d="$PI_CODING_AGENT_DIR"; [ -z "$d" ] && d="$HOME/.pi/agent"; if [ -f "$d/ready-marker" ]; then echo '{"status":"ready","provider":"synthetic"}'; else echo '{"status":"not_ready","provider":"synthetic","reason":"credentials_not_configured"}'; fi; exit 0"#,
    );
    let home = fixture.root.join("home");
    std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
    std::fs::write(
        home.join(".pi/agent/settings.json"),
        r#"{"defaultProvider":"homeprov"}"#,
    )
    .unwrap();
    std::fs::write(
        home.join(".pi/agent/models.json"),
        r#"{"providers":{"homeprov":{"models":[{"id":"hm"}]}}}"#,
    )
    .unwrap();
    let override_dir = fixture.root.join("alt-agent");
    std::fs::create_dir_all(&override_dir).unwrap();
    std::fs::write(
        override_dir.join("settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"om"}"#,
    )
    .unwrap();
    std::fs::write(
        override_dir.join("models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"om"}]}}}"#,
    )
    .unwrap();
    std::fs::write(override_dir.join("ready-marker"), "signed-in").unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", home.as_os_str());
    environment.set("PI_CODING_AGENT_DIR", override_dir.as_os_str());
    environment.set(
        crate::harness::pi_harness::PI_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = super::super::discover_provider("pi", true);
    // With the override dropped, the fake resolves $HOME/.pi/agent (where no
    // ready-marker exists) and reports credentials_not_configured.
    assert_eq!(report.authentication, ReadinessState::Ready, "{report:?}");
    assert!(report.model_catalog_fresh, "{report:?}");
    assert_eq!(report.models, vec!["om".to_owned()]);
    assert_eq!(
        fixture.invoked_auth_args(),
        Some(vec![
            "auth".to_owned(),
            "check".to_owned(),
            "--provider".to_owned(),
            "synthetic".to_owned(),
            "--json".to_owned(),
            "--no-refresh".to_owned(),
        ])
    );
}
