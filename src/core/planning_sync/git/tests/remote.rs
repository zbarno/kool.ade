use super::super::{audit_local_config, is_remote_helper, safe_command};
use super::support::{init_repo, test_dir};
use std::process::Command;

#[test]
fn rejects_multi_destination_and_mirror_push_configuration() {
    let temp = test_dir();
    init_repo(&temp);
    for url in ["https://first.invalid/repo", "https://second.invalid/repo"] {
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    temp.to_str().unwrap(),
                    "config",
                    "--local",
                    "--add",
                    "remote.origin.pushurl",
                    url,
                ])
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(audit_local_config(&temp).is_err());
    assert!(
        Command::new("git")
            .args([
                "-C",
                temp.to_str().unwrap(),
                "config",
                "--local",
                "--unset-all",
                "remote.origin.pushurl",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-C",
                temp.to_str().unwrap(),
                "config",
                "--local",
                "remote.origin.mirror",
                "true",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(audit_local_config(&temp).is_err());
    let _ = std::fs::remove_dir_all(temp);
}

#[test]
fn allows_ipv6_ssh_and_https_remotes() {
    assert!(!is_remote_helper("ssh://git@[2001:db8::1]/repo"));
    assert!(!is_remote_helper("https://[2001:db8::1]/repo"));
}

#[test]
fn safe_git_commands_strip_packfile_trace_redirection() {
    assert!(
        safe_command(None)
            .get_envs()
            .any(|(key, value)| key == "GIT_TRACE_PACKFILE" && value.is_none())
    );
}

#[test]
fn permits_one_explicitly_configured_push_destination() {
    let temp = test_dir();
    init_repo(&temp);
    assert!(
        Command::new("git")
            .args([
                "-C",
                temp.to_str().unwrap(),
                "config",
                "--local",
                "remote.origin.pushurl",
                "https://alternate.invalid/repo",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(audit_local_config(&temp).is_ok());
    let _ = std::fs::remove_dir_all(temp);
}
