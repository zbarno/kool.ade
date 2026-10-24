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
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("codex login"));
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let report = CodexHarness::probe_report();
    assert_eq!(report.readiness, CodexReadiness::AuthenticationRequired);
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_codex_is_ready_when_login_confirmation_is_on_stderr() {
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

    assert_eq!(CodexHarness::check_binary(&binary).unwrap(), "0.160.1");
    let _env = EnvOverride::set(CODEX_BINARY_ENV, &binary);
    let report = CodexHarness::probe_report();
    assert!(report.ready);
    assert_eq!(report.readiness, CodexReadiness::Ready);
    assert_eq!(report.version.as_deref(), Some("0.160.1"));

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
    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("login status could not be checked"));
    assert!(!error.detail().contains("codex login"));
    std::fs::remove_dir_all(root).unwrap();
}
