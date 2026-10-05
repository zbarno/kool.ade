use super::*;

#[test]
fn output_locations_do_not_include_repository_configuration() {
    assert!(eligible("Source/Core/obj/generated.cs"));
    assert!(eligible("Source/App/bin/Debug/app.dll"));
    assert!(eligible("ClientApp/node_modules/demo/index.js"));
    assert!(eligible("App/DataProtectionKeys/key.xml"));
    assert!(!eligible("Source/Directory.Build.props"));
    assert!(!eligible("Source/Core/new-source.cs"));
    assert!(!eligible(".env"));
    assert!(!eligible("App/DataProtectionKeys/config.json"));
}

#[test]
fn fingerprints_allow_internal_links_but_reject_traversal_and_external_links() {
    let root = std::env::temp_dir().join(format!(
        "koolade-generated-fixture-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(root.join(".cache")).unwrap();
    fs::write(root.join(".cache/output.txt"), "verification output").unwrap();
    let initial = fingerprint(&root, ".cache/output.txt").unwrap();
    assert!(initial.is_some());
    fs::write(root.join(".cache/output.txt"), "operator edit").unwrap();
    assert_ne!(fingerprint(&root, ".cache/output.txt").unwrap(), initial);
    assert!(fingerprint(&root, "../elsewhere").is_err());
    std::os::unix::fs::symlink("output.txt", root.join(".cache/link.txt")).unwrap();
    assert!(fingerprint(&root, ".cache/link.txt").unwrap().is_some());
    std::os::unix::fs::symlink(root.join(".cache"), root.join("linked-cache")).unwrap();
    assert_eq!(fingerprint(&root, "linked-cache/output.txt").unwrap(), None);
    let outside = root.with_extension("outside");
    fs::write(&outside, "outside the worktree").unwrap();
    let relative_outside = format!("../../{}", outside.file_name().unwrap().to_str().unwrap());
    std::os::unix::fs::symlink(relative_outside, root.join(".cache/external.txt")).unwrap();
    assert_eq!(fingerprint(&root, ".cache/external.txt").unwrap(), None);
    fs::remove_file(outside).unwrap();
    fs::remove_dir_all(root).unwrap();
}
