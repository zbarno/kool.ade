use super::*;

#[cfg(unix)]
#[test]
fn executable_override_is_validated_without_leaking_override_value() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("koolade-codex-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(executable(&binary));
    assert!(!executable(&root));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_codex_without_authentication_is_reported_as_setup_required() {
    let root = std::env::temp_dir().join(format!("koolade-codex-auth-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; else echo 'Not logged in'; exit 1; fi\n",
    ).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("codex login"));
    let report = CodexHarness::probe_report();
    assert_eq!(report.readiness, CodexReadiness::AuthenticationRequired);
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn logged_in_codex_stays_unverified_until_the_bounded_live_check_passes() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-auth-stderr-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let verify_called = root.join("verify-called");
    let login_called = root.join("login-called");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 0.160.1'\nelif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'\nelif [ \"$1\" = \"exec\" ]; then touch '{v}'; echo 'verification declined: no model entitlement' >&2; exit 1\nelif [ \"$1\" = \"login\" ]; then touch '{l}'; echo 'Logged in as tester@example.com with ChatGPT'; exit 0\nelse exit 2\nfi\n",
            v = verify_called.display(),
            l = login_called.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let report = CodexHarness::probe_report();
    assert!(!report.ready);
    assert_eq!(report.readiness, CodexReadiness::AuthenticationUnverified);
    assert_eq!(report.version.as_deref(), Some("0.160.1"));
    assert!(report.verified_at_unix_ms.is_none());
    // A login sighting alone no longer proves access: the probe spent one
    // bounded live check on it, and that check is reflected in diagnostics.
    assert!(verify_called.exists());
    assert!(login_called.exists());
    assert!(
        report
            .diagnostic
            .contains("Codex account and model access remain unverified")
    );
    assert!(report.diagnostic.contains("live verification failed"));
    assert!(report.diagnostic.contains("no model entitlement"));
    // Usernames from the login banner must not be echoed into diagnostics.
    assert!(!report.diagnostic.contains("tester@example.com"));
    assert!(!report.status.contains("tester@example.com"));
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(
        error
            .detail()
            .contains("account and model access remain unverified")
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn codex_api_key_login_status_stays_unverified_and_redacted() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-api-key-status-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let login_called = root.join("login-called");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'\nelif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'\nelif [ \"$1\" = \"exec\" ]; then echo 'access denied (quota suspended)' >&2; exit 1\nelif [ \"$1\" = \"login\" ]; then touch '{l}'; echo 'API key configured - sk-synthetic-secret'; exit 0\nelse exit 2\nfi\n",
            l = login_called.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);

    let report = CodexHarness::probe_report();
    assert!(!report.ready);
    assert_eq!(report.readiness, CodexReadiness::AuthenticationUnverified);
    assert!(report.verified_at_unix_ms.is_none());
    // The CLI banner carried the key; classification must never forward it.
    assert!(login_called.exists());
    assert!(!report.status.contains("sk-synthetic-secret"));
    assert!(!report.diagnostic.contains("sk-synthetic-secret"));
    assert!(report.diagnostic.contains("live verification failed"));
    assert!(
        report
            .diagnostic
            .contains("access denied (quota suspended)")
    );
    assert!(
        report
            .diagnostic
            .contains("Codex reports an API key configured")
    );
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(
        error
            .detail()
            .contains("account and model access remain unverified")
    );
    assert!(!error.detail().contains("sk-synthetic-secret"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn codex_environment_credentials_skip_login_status_but_still_verify_live() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-auth-environment-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let login_called = root.join("login-status-called");
    let verify_called = root.join("verify-called");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'\nelif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'\nelif [ \"$1\" = \"exec\" ]; then touch '{v}'; echo 'request denied' >&2; exit 1\nelif [ \"$1\" = \"login\" ]; then touch '{l}'; echo 'Logged in'; exit 0\nelse exit 2\nfi\n",
            v = verify_called.display(),
            l = login_called.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = clean_codex_auth_environment();
    environment.set(CODEX_BINARY_ENV, binary.as_os_str());
    environment.set("OPENAI_FEDERATION_RULE_ID", "synthetic-rule");

    let workload_identity_report = CodexHarness::probe_report();
    assert_eq!(
        workload_identity_report.readiness,
        CodexReadiness::AuthenticationUnverified
    );
    assert!(!workload_identity_report.ready);
    // Environment-selected credentials skip the login-status call ...
    assert!(!login_called.exists());
    // ... but a failed account can still spend the bounded live check.
    assert!(verify_called.exists());
    assert!(CodexHarness::check_binary(&binary).is_err());
    std::fs::remove_file(&verify_called).unwrap();

    environment.remove("OPENAI_FEDERATION_RULE_ID");
    environment.set("OPENAI_API_KEY", "synthetic-openai-key");
    let api_key_report = CodexHarness::probe_report();
    assert_eq!(
        api_key_report.readiness,
        CodexReadiness::AuthenticationUnverified
    );
    assert!(!api_key_report.ready);
    assert!(!api_key_report.status.contains("synthetic-openai-key"));
    assert!(!api_key_report.diagnostic.contains("synthetic-openai-key"));
    assert!(!login_called.exists());
    assert!(verify_called.exists());
    assert!(CodexHarness::check_binary(&binary).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_codex_missing_required_exec_options_is_unusable() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("koolade-codex-old-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 0.1.0'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd'; else echo 'Logged in'; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _environment = clean_codex_auth_environment();
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("required exec options"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn codex_login_status_execution_failure_is_not_mislabeled_as_authentication() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-login-error-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; else exit 2; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _environment = clean_codex_auth_environment();
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("login status could not be checked"));
    assert!(!error.detail().contains("codex login"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
fn seed_codex_detection(verified_at_unix_ms: Option<u64>) {
    let (mut settings, _) = crate::persistence::harness_settings::load();
    settings.discovered.insert(
        "codex".into(),
        crate::persistence::harness_settings::DetectedHarness {
            status: "codex 1.2.3".into(),
            version: Some("1.2.3".into()),
            executable: Some("/synthetic/bin/codex".into()),
            diagnostic: None,
            ready: verified_at_unix_ms.is_some(),
            models: vec!["gpt-verification".into()],
            default_model: Some("gpt-verification".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            implementation_available: false,
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
            verified_at_unix_ms,
            capability_snapshot: None,
        },
    );
    crate::persistence::harness_settings::save(&settings).expect("seed settings store");
}

#[cfg(unix)]
#[test]
fn stored_verification_receipt_avoids_repeat_live_calls() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("koolade-codex-receipt-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let call_count = root.join("verify-calls");
    std::fs::write(&call_count, "").unwrap();
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'\nelif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'\nelif [ \"$1\" = \"exec\" ]; then echo >> '{c}'; echo 'ok'; exit 0\nelif [ \"$1\" = \"login\" ]; then echo 'Logged in as tester@example.com with ChatGPT'; exit 0\nelse exit 2\nfi\n",
            c = call_count.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);

    let before = crate::persistence::harness_settings::current_unix_ms();
    let first = CodexHarness::probe_report();
    let after = crate::persistence::harness_settings::current_unix_ms();
    assert_eq!(first.readiness, CodexReadiness::Ready);
    assert!(first.ready);
    let Some(stamp) = first.verified_at_unix_ms else {
        panic!("a successful live verification must carry a receipt stamp");
    };
    assert!((before..=after).contains(&stamp));
    assert_eq!(
        std::fs::read_to_string(&call_count)
            .unwrap()
            .lines()
            .count(),
        1,
        "exactly one bounded live call is allowed"
    );

    // Persist the receipt the way discovery does, then re-prove readiness
    // without spending another model call.
    seed_codex_detection(Some(stamp));
    let second = CodexHarness::probe_report();
    assert_eq!(second.readiness, CodexReadiness::Ready);
    assert!(second.ready);
    assert_eq!(second.verified_at_unix_ms, Some(stamp));
    assert_eq!(
        std::fs::read_to_string(&call_count)
            .unwrap()
            .lines()
            .count(),
        1,
        "a current receipt must suppress further live calls"
    );
    // The execution gate honors the same persisted evidence.
    assert!(CodexHarness::check_binary(&binary).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn expired_verification_receipt_requires_new_live_call() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-receipt-expired-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let verify_called = root.join("verify-called");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'\nelif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'\nelif [ \"$1\" = \"exec\" ]; then touch '{v}'; echo 'lease expired: verification unavailable' >&2; exit 1\nelif [ \"$1\" = \"login\" ]; then echo 'Logged in as tester@example.com with ChatGPT'; exit 0\nelse exit 2\nfi\n",
            v = verify_called.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);

    // Receipt older than the 24 hour validity window.
    let now = crate::persistence::harness_settings::current_unix_ms();
    let ttl = 24 * 60 * 60 * 1000u64;
    seed_codex_detection(Some(now.saturating_sub(ttl + 10_000)));

    let report = CodexHarness::probe_report();
    assert_eq!(report.readiness, CodexReadiness::AuthenticationUnverified);
    assert!(!report.ready);
    assert!(report.verified_at_unix_ms.is_none());
    assert!(
        verify_called.exists(),
        "an expired receipt must be refreshed with a fresh live check"
    );
    assert!(report.diagnostic.contains("live verification failed"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sensitive_verifier_output_is_masked_in_diagnostics() {
    let masked =
        super::super::probe::scrub_sensitive("401 token sk-abcdef123456ghijkl is invalid".into());
    assert!(masked.contains("[REDACTED]"));
    assert!(!masked.contains("sk-abcdef123456ghijkl"));
    let innocent = super::super::probe::scrub_sensitive("risk-based skim of disk-usage".into());
    assert_eq!(innocent, "risk-based skim of disk-usage");
}
