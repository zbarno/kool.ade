use super::super::{audit_local_config, classify_transport};
use super::support::{init_repo, test_dir};
use std::process::Command;

#[test]
fn ssh_public_key_rejection_is_an_authentication_failure() {
    assert_eq!(
        classify_transport("permission denied (publickey)."),
        "credentials are unavailable"
    );
}

#[test]
fn rejects_local_git_settings_that_can_execute_or_redirect_commands() {
    let temp = test_dir();
    init_repo(&temp);
    std::fs::create_dir_all(temp.join("hooks")).unwrap();
    std::fs::write(temp.join("hooks/pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "core.hooksPath",
            "hooks",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_local_config_includes_without_loading_them() {
    let temp = test_dir();
    init_repo(&temp);
    let include = temp.join("untrusted-gitconfig");
    std::fs::write(&include, "[core]\n hooksPath = ../hooks\n").unwrap();
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "include.path",
            include.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_url_scoped_credential_helpers() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "credential.https://example.invalid.helper",
            "!echo injected-helper",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_conditional_local_config_includes() {
    let temp = test_dir();
    init_repo(&temp);
    let config_path = temp.join(".git/config");
    let mut config = std::fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[includeIf \"gitdir:**\"]\n path = untrusted-gitconfig\n");
    std::fs::write(config_path, config).unwrap();
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_local_askpass_commands() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "core.askPass",
            "echo injected-askpass",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_external_git_remote_helpers() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "remote.origin.url",
            "ext::echo injected-remote",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_option_looking_git_remotes() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "remote.origin.url",
            "--upload-pack=echo injected",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_unknown_git_remote_schemes() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "remote.origin.url",
            "untrusted://example.invalid/repository",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}

#[test]
fn rejects_named_diff_driver_commands_and_worktree_config() {
    let temp = test_dir();
    init_repo(&temp);
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "diff.review.command",
            "echo injected-diff",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    assert!(
        Command::new("git")
            .args([
                "-C",
                temp.to_str().unwrap(),
                "config",
                "--local",
                "--unset-all",
                "diff.review.command",
            ])
            .status()
            .unwrap()
            .success()
    );
    let config = Command::new("git")
        .args([
            "-C",
            temp.to_str().unwrap(),
            "config",
            "--local",
            "extensions.worktreeConfig",
            "true",
        ])
        .output()
        .unwrap();
    assert!(config.status.success());
    assert!(audit_local_config(&temp).is_err());
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn rejects_proxy_helper_tls_and_signing_overrides() {
    let temp = test_dir();
    init_repo(&temp);
    for (key, value) in [
        ("core.gitProxy", "echo injected-proxy"),
        ("remote.origin.vcs", "untrusted"),
        ("remote.origin.uploadpack", "echo injected-uploadpack"),
        ("remote.origin.receivepack", "echo injected-receivepack"),
        ("http.sslVerify", "false"),
        ("http.proxy", "http://proxy.invalid"),
        ("commit.gpgSign", "true"),
        ("gpg.program", "echo injected-signer"),
    ] {
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    temp.to_str().unwrap(),
                    "config",
                    "--local",
                    key,
                    value
                ])
                .status()
                .unwrap()
                .success()
        );
        assert!(audit_local_config(&temp).is_err(), "accepted {key}");
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    temp.to_str().unwrap(),
                    "config",
                    "--local",
                    "--unset-all",
                    key
                ])
                .status()
                .unwrap()
                .success()
        );
    }
    let _ = std::fs::remove_dir_all(temp);
}
