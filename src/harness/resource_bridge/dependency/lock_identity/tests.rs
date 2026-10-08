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

#[test]
fn new_npm_add_rejects_a_preedited_lockfile_package_before_authorization() {
    let root = baseline_project();
    let baseline = baseline_commit(&root).unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{"lockfileVersion":3,"packages":{"node_modules/other-package":{"version":"1.0.0","resolved":"https://registry.npmjs.org/other-package/-/other-package-1.0.0.tgz","integrity":"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="}}}"#,
    )
    .unwrap();
    let mut need = new_npm_add();

    let error = enrich(&root, Some(&baseline), &mut need).unwrap_err();

    assert!(error.to_string().contains("changed before authorization"));
    assert!(need.lockfile_identity.is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_npm_add_rejects_a_preedited_manifest_dependency_before_authorization() {
    let root = baseline_project();
    let baseline = baseline_commit(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","dependencies":{"other-package":"^1.0.0"}}"#,
    )
    .unwrap();
    let mut need = new_npm_add();

    let error = enrich(&root, Some(&baseline), &mut need).unwrap_err();

    assert!(error.to_string().contains("changed before authorization"));
    assert!(need.lockfile_identity.is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn npm_add_snapshot_is_validated_again_after_authorization() {
    let root = baseline_project();
    let baseline = baseline_commit(&root).unwrap();
    let mut need = new_npm_add();
    enrich(&root, Some(&baseline), &mut need).unwrap();
    assert!(crate::harness::dependency_decision_allowed(
        &need,
        crate::harness::DependencyDecision::AutoAuthorize
    ));

    fs::write(
        root.join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","devDependencies":{"other-package":"^1.0.0"}}"#,
    )
    .unwrap();

    assert!(
        validate_current_identity(&root, &need)
            .unwrap_err()
            .to_string()
            .contains("changed after authorization")
    );
    fs::remove_dir_all(root).unwrap();
}

fn baseline_project() -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("koolade-npm-add-baseline-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{"lockfileVersion":3,"packages":{"":{"name":"fixture","version":"1.0.0"}}}"#,
    )
    .unwrap();
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "package.json", "package-lock.json"]);
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
            "baseline project",
        ],
    );
    root
}

fn new_npm_add() -> DependencyNeed {
    DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: Some("zod".into()),
        version: Some("^4.0.0".into()),
        source: Some("https://registry.npmjs.org/".into()),
        command: "npm install zod@^4.0.0".into(),
        reason: "Validate imported settings data".into(),
        kind: DependencyKind::NewProjectDependency,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    }
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
