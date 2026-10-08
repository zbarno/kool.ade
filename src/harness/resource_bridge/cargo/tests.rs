use super::{locate_cargo_in_path, run_fetch};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, fs::symlink},
    path::PathBuf,
};

#[test]
fn cargo_proxy_symlink_keeps_its_alias_when_invoked_for_fetch() {
    let root = std::env::temp_dir().join(format!(
        "koolade-cargo-proxy-alias-{}",
        uuid::Uuid::new_v4()
    ));
    let worktree = root.join("worktree");
    let home = root.join("home");
    let bin = home.join(".cargo/bin");
    let cache = root.join("cache");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&cache).unwrap();
    let rustup = bin.join("rustup");
    let cargo = bin.join("cargo");
    fs::write(
        &rustup,
        "#!/bin/sh\nprintf '%s\\n%s\\n' \"$0\" \"$*\" > \"$CARGO_HOME/invocation\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&rustup, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&rustup, &cargo).unwrap();

    let path = std::env::join_paths([bin.as_path()]).unwrap();
    let located =
        locate_cargo_in_path(&worktree, &path, Some(&home.canonicalize().unwrap())).unwrap();
    assert_eq!(located, cargo);
    assert_ne!(located, rustup.canonicalize().unwrap());

    run_fetch(&located, &worktree, &cache, "http://127.0.0.1:1").unwrap();
    let invocation = fs::read_to_string(cache.join("invocation")).unwrap();
    let mut lines = invocation.lines();
    assert_eq!(PathBuf::from(lines.next().unwrap()), cargo);
    assert!(lines.next().unwrap().contains("--locked fetch"));
    fs::remove_dir_all(root).unwrap();
}
