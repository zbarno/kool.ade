use super::*;

#[cfg(unix)]
#[test]
fn codex_without_clean_launch_controls_is_unsupported() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "koolade-codex-capabilities-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; elif [ \"$1\" = \"exec\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --ignore-user-config --skip-git-repo-check'; else echo 'Logged in'; fi\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = CodexHarness::check_binary(&binary).unwrap_err();
    assert!(error.detail().contains("--disable"));

    std::fs::remove_dir_all(root).unwrap();
}
