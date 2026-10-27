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
fn successful_codex_login_status_does_not_prove_account_access() {
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-auth-stderr-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 0.160.1'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; else echo 'Logged in' >&2; fi\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(
        error
            .detail()
            .contains("account and model access remain unverified")
    );
    let report = CodexHarness::probe_report();
    assert!(!report.ready);
    assert_eq!(report.readiness, CodexReadiness::AuthenticationUnverified);
    assert_eq!(report.version.as_deref(), Some("0.160.1"));
    assert!(
        report
            .diagnostic
            .contains("no generative model call was made")
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
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; elif [ \"$1\" = \"login\" ]; then echo 'API key configured - sk-synthetic-secret'; else exit 2; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);

    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(
        error
            .detail()
            .contains("account and model access remain unverified")
    );
    let report = CodexHarness::probe_report();
    assert!(!report.ready);
    assert_eq!(report.readiness, CodexReadiness::AuthenticationUnverified);
    assert!(!report.status.contains("sk-synthetic-secret"));
    assert!(!report.diagnostic.contains("sk-synthetic-secret"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn codex_auth_probe_skips_status_for_workload_identity_and_api_key_environment() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-auth-environment-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    let login_called = root.join("login-status-called");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; elif [ \"$1\" = \"login\" ]; then touch '{}'; echo 'Logged in'; exit 0; else exit 2; fi\n",
            login_called.display()
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
    assert!(CodexHarness::check_binary(&binary).is_err());
    assert!(!login_called.exists());

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
    assert!(CodexHarness::check_binary(&binary).is_err());
    assert!(!login_called.exists());
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
