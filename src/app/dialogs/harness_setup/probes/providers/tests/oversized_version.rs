use super::*;

/// Exercises each adapter through its actual executable-selection and output-mapping path.
/// The supervisor unit tests prove byte limits in isolation; this verifies none of the six
/// adapters converts truncated version output into a ready provider. It needs only `/bin/sh`
/// plus `head`/`tr` and a temporary file, with one bounded 1 MiB subprocess per provider.
#[test]
fn oversized_version_output_is_a_provider_local_failure_for_all_six_adapters() {
    let root = std::env::temp_dir().join(format!(
        "koolade-provider-oversized-version-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("fake-cli");
    std::fs::write(
        &binary,
        "#!/bin/sh\nprintf 'provider 1.2.3\\n'\nhead -c 1048577 /dev/zero | tr '\\000' x\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    for variable in [
        crate::harness::pi_harness::PI_BINARY_ENV,
        crate::harness::codex_harness::CODEX_BINARY_ENV,
        crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        crate::harness::copilot_harness::COPILOT_BINARY_ENV,
        crate::harness::antigravity_harness::ANTIGRAVITY_BINARY_ENV,
    ] {
        environment.set(variable, binary.as_os_str());
    }

    for provider in PROVIDERS {
        let report = discover_provider(provider, true);
        assert_eq!(report.id, provider);
        assert!(
            !report.ready,
            "{provider} accepted truncated version output"
        );
        assert!(
            report.executable.is_some(),
            "{provider} lost installed identity"
        );
        assert_eq!(
            report.executable.as_deref(),
            Some(binary.to_string_lossy().as_ref()),
            "{provider} should probe the synthetic CLI selected by this test"
        );
        assert!(
            report
                .diagnostic
                .as_ref()
                .is_some_and(|diagnostic| diagnostic.len() <= 2048),
            "{provider} should report a bounded failed-probe diagnostic"
        );
        let result =
            super::super::super::super::probe_result::from_harness_probe(&report, 9, 10, 1);
        assert_eq!(
            result.status,
            crate::harness::discovery::ProbeStatus::Installed,
            "{provider} must remain installed rather than being mislabeled missing"
        );
        assert!(result.diagnostic_message.is_some());
    }

    std::fs::remove_dir_all(root).unwrap();
}
