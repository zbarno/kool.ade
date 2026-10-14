use super::*;

#[test]
fn trust_pins_only_the_operator_verified_host_and_revoke_removes_it() {
    let root = tempfile::tempdir().unwrap();
    let fingerprint = format!("SHA256:{}", "A".repeat(43));
    create_private_dir(&root.path().join("remote-workers")).unwrap();
    fs::write(
        known_hosts_path(root.path()),
        "worker-west ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixtureOtherWorker\n",
    )
    .unwrap();
    trust_at(
        root.path(),
        "worker-east",
        "build-host",
        "worker-user",
        &fingerprint,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixturePublicKeyOnly",
    )
    .unwrap();

    let registration = load_from(root.path(), "worker-east").unwrap().unwrap();
    assert_eq!(registration.host_key_fingerprint, fingerprint);
    assert_eq!(
        fs::read_to_string(known_hosts_path(root.path())).unwrap(),
        "worker-west ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixtureOtherWorker\nworker-east ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixturePublicKeyOnly\n"
    );

    revoke_at(root.path(), "worker-east").unwrap();
    assert!(load_from(root.path(), "worker-east").unwrap().is_none());
    assert_eq!(
        fs::read_to_string(known_hosts_path(root.path())).unwrap(),
        "worker-west ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixtureOtherWorker"
    );
}

#[test]
fn host_key_scan_must_match_the_entered_fingerprint() {
    let root = tempfile::tempdir().unwrap();
    let key_path = root.path().join("host-key");
    assert!(
        std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-f"])
            .arg(&key_path)
            .status()
            .unwrap()
            .success()
    );
    let public = fs::read_to_string(key_path.with_extension("pub")).unwrap();
    let fields = public.split_whitespace().collect::<Vec<_>>();
    let scan_line = format!("build-host {} {}", fields[0], fields[1]);
    let output = std::process::Command::new("ssh-keygen")
        .args(["-lf"])
        .arg(key_path.with_extension("pub"))
        .output()
        .unwrap();
    let fingerprint = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    assert_eq!(
        matching_key(&scan_line, &fingerprint).unwrap(),
        format!("{} {}", fields[0], fields[1])
    );
    assert!(matching_key(&scan_line, &format!("SHA256:{}", "A".repeat(43))).is_err());
}

#[test]
fn trust_rejects_invalid_worker_names_and_known_hosts_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let fingerprint = format!("SHA256:{}", "A".repeat(43));
    assert!(
        trust_at(
            root.path(),
            "../worker",
            "build-host",
            "worker-user",
            &fingerprint,
            "ssh-ed25519 AAAA"
        )
        .is_err()
    );

    let path = known_hosts_path(root.path());
    create_private_dir(path.parent().unwrap()).unwrap();
    let target = root.path().join("other");
    fs::write(&target, "").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &path).unwrap();
    #[cfg(unix)]
    assert!(read_known_hosts(&path).is_err());
}
