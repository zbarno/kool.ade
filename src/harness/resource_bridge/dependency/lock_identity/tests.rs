use super::*;
use crate::harness::{DependencyKind, DependencyNeed};
use std::{fs, path::Path, process::Command};

#[test]
fn baseline_package_scan_uses_the_current_npm_lockfile_exclusions() {
    let root = std::env::temp_dir().join(format!(
        "koolade-baseline-lock-exclusions-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(root.join("vendor")).unwrap();
    let lock = r#"{"lockfileVersion":3,"packages":{"node_modules/zod":{"version":"4.0.0","resolved":"https://registry.npmjs.org/zod/-/zod-4.0.0.tgz","integrity":"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="}}}"#;
    fs::write(root.join("vendor/package-lock.json"), lock).unwrap();
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "vendor/package-lock.json"]);
    git(
        &root,
        &[
            "-c",
            "user.name=Synthetic Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "baseline vendor lockfile",
        ],
    );
    let baseline = baseline_commit(&root).unwrap();
    fs::write(root.join("package-lock.json"), lock).unwrap();
    let mut need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some("https://registry.npmjs.org".into()),
        command: "npm ci".into(),
        reason: "Restore locked task dependencies".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };

    enrich(&root, Some(&baseline), &mut need).unwrap();

    assert_eq!(need.introduced_packages.len(), 1);
    assert_eq!(need.introduced_packages[0].package, "zod");
    fs::remove_dir_all(root).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
