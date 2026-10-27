use super::*;

fn manifest(
    name: &str,
    kind: &str,
    uid: &str,
    gid: &str,
    mode: &str,
    size: &str,
    links: &str,
) -> String {
    format!("{name}\0{kind}\0{uid}\0{gid}\0{mode}\0{size}\0{links}\0")
}

#[test]
fn package_manifest_accepts_only_bounded_root_owned_regular_rpms() {
    let stage = DnfStageId::new();
    let output = manifest(
        "docker-ce-1.0-1.x86_64.rpm",
        "f",
        "0",
        "0",
        "600",
        "123",
        "1",
    );

    let packages = parse_package_manifest(&output, &stage).unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].basename(), "docker-ce-1.0-1.x86_64.rpm");
}

#[test]
fn package_manifest_rejects_empty_symlink_writable_unowned_and_duplicate_entries() {
    let stage = DnfStageId::new();
    for invalid in [
        manifest("one.rpm", "l", "0", "0", "600", "10", "1"),
        manifest("one.rpm", "f", "1", "0", "600", "10", "1"),
        manifest("one.rpm", "f", "0", "0", "666", "10", "1"),
        manifest("one.rpm", "f", "0", "0", "600", "0", "1"),
        manifest("one.rpm", "f", "0", "0", "600", "10", "2"),
        manifest("../one.rpm", "f", "0", "0", "600", "10", "1"),
    ] {
        assert!(parse_package_manifest(&invalid, &stage).is_err());
    }
    let duplicate = format!(
        "{}{}",
        manifest("one.rpm", "f", "0", "0", "600", "10", "1"),
        manifest("one.rpm", "f", "0", "0", "600", "10", "1")
    );
    assert!(parse_package_manifest(&duplicate, &stage).is_err());
}

#[test]
fn sha256_output_binds_digest_to_the_exact_stage_path() {
    let stage = DnfStageId::new();
    let package = stage.package("docker-ce-1.0-1.x86_64.rpm").unwrap();
    let digest = "a".repeat(64);
    let output = ProcessOutput {
        exit_code: 0,
        stdout: format!("{digest} *{}\n", package.path().display()),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    };

    let hash_bound = parse_sha256_output(&output, std::slice::from_ref(&package)).unwrap();
    assert_eq!(hash_bound.len(), 1);
    assert_eq!(hash_bound[0].package(), &package);
    assert!(hash_bound[0].binding_value().ends_with(&digest));

    let wrong_path = ProcessOutput {
        stdout: format!("{digest} */tmp/other.rpm\n"),
        ..output
    };
    assert!(parse_sha256_output(&wrong_path, &[package]).is_err());
}
