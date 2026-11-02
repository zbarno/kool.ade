use super::{ApprovedProjectGuidanceV1, load_for_root, save_for_root, store_path};
use std::{fs, process::Command};

struct Fixture {
    root: tempfile::TempDir,
    repo: std::path::PathBuf,
    _environment: crate::test_support::ProcessEnvironmentGuard,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        environment.set("KOOLADE_HOME", root.path().join("state"));
        let repo = root.path().join("code");
        fs::create_dir(&repo).unwrap();
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(output.status.success());
        Self {
            root,
            repo,
            _environment: environment,
        }
    }
}

#[test]
fn operator_approval_binds_exact_content_and_project_identity() {
    let fixture = Fixture::new();
    let text = "Keep the Rust implementation formatted with rustfmt.";

    let receipt = save_for_root(&fixture.repo, text, true).unwrap().unwrap();
    assert_eq!(receipt.content, text);
    assert_eq!(
        receipt.source_path,
        "koolade://project-settings/approved-agent-guidance"
    );
    assert_eq!(load_for_root(&fixture.repo).unwrap(), Some(receipt.clone()));

    let changed = save_for_root(
        &fixture.repo,
        "Keep Rust changes formatted and linted.",
        true,
    )
    .unwrap()
    .unwrap();
    assert_ne!(changed.receipt_uid, receipt.receipt_uid);
    assert_ne!(changed.content_sha256, receipt.content_sha256);
    assert_eq!(load_for_root(&fixture.repo).unwrap(), Some(changed));

    let other = fixture.root.path().join("other-code");
    fs::create_dir(&other).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&other)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(load_for_root(&other).unwrap(), None);
}

#[test]
fn disabled_or_modified_guidance_is_not_available_to_prompts() {
    let fixture = Fixture::new();
    save_for_root(&fixture.repo, "Approved convention.", true).unwrap();
    save_for_root(&fixture.repo, "Approved convention.", false).unwrap();
    assert_eq!(load_for_root(&fixture.repo).unwrap(), None);

    let project_id =
        crate::persistence::dependency_authorization::project_id(&fixture.repo).unwrap();
    let path = store_path(&project_id);
    let mut receipt: ApprovedProjectGuidanceV1 =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt.enabled = true;
    receipt.content.push_str(" changed after approval");
    fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(load_for_root(&fixture.repo).is_err());
}
