use std::fs;

use super::support::{Fixture, git};

#[test]
fn divergent_branch_records_stop_before_cutover_and_write_source_hash_conflicts() {
    let fixture = Fixture::new("branch-divergence");
    let work_path = fixture.code.join(".koolade-packet/state/work.json");
    git(
        &fixture.code,
        &["switch", "--quiet", "-c", "feature/divergent"],
    );
    let mut divergent = fs::read_to_string(&work_path).unwrap();
    divergent = divergent.replace(
        "Keep all current planning",
        "Drop the approval and change the record",
    );
    fs::write(&work_path, divergent).unwrap();
    git(&fixture.code, &["add", ".koolade-packet/state/work.json"]);
    git(
        &fixture.code,
        &["commit", "--quiet", "-m", "divergent embedded work"],
    );
    git(&fixture.code, &["switch", "--quiet", "main"]);

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning sources contain divergent")
    );
    let conflict_path = fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "json")
                && path.to_string_lossy().contains("conflicts")
        })
        .expect("structured conflict manifest");
    let conflicts: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict_path).unwrap()).unwrap();
    let conflict = conflicts["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|conflict| conflict["artifactPath"] == ".koolade-packet/state/work.json")
        .expect("work record conflict");
    assert_eq!(conflict["kind"], "same-path-divergence");
    assert_eq!(conflict["recordId"], fixture.work_uid);
    assert_eq!(conflict["sources"].as_array().unwrap().len(), 2);
    assert!(
        conflict["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|source| source["sha256"].as_str().is_some_and(|sha| sha.len() == 64))
    );
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
    assert!(
        !fs::read_dir(fixture.root.join("operator/projects"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.path().join("association.json").exists())
    );
}

#[test]
fn divergent_branch_workspace_identity_stops_migration() {
    let fixture = Fixture::new("branch-workspace-identity");
    let manifest_path = fixture.code.join(".koolade-packet/manifest.json");
    git(
        &fixture.code,
        &["switch", "--quiet", "-c", "feature/workspace-identity"],
    );
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["projectId"] =
        serde_json::Value::String(uuid::Uuid::new_v4().hyphenated().to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    git(&fixture.code, &["add", ".koolade-packet/manifest.json"]);
    git(
        &fixture.code,
        &[
            "commit",
            "--quiet",
            "-m",
            "divergent legacy workspace identity",
        ],
    );
    git(&fixture.code, &["switch", "--quiet", "main"]);

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning sources contain divergent")
    );
    let conflict_path = fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("conflicts"))
        .expect("workspace identity conflict manifest");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict_path).unwrap()).unwrap();
    assert!(
        report["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| {
                conflict["kind"] == "workspace-identity-divergence"
                    && conflict["sources"].as_array().is_some_and(|sources| {
                        sources.len() == 2
                            && sources.iter().all(|source| {
                                source["sha256"]
                                    .as_str()
                                    .is_some_and(|hash| hash.len() == 64)
                            })
                    })
            })
    );
    assert_eq!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap(),
        None
    );
}

#[test]
fn divergent_linked_worktree_snapshot_is_reported_without_checkout_or_reset() {
    let fixture = Fixture::new("linked-worktree-divergence");
    let worktree = fixture.root.join("feature-worktree");
    git(
        &fixture.code,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature/divergent",
            worktree.to_str().unwrap(),
        ],
    );
    let work_path = worktree.join(".koolade-packet/state/work.json");
    let original = fs::read(&work_path).unwrap();
    let mut divergent = String::from_utf8(original.clone()).unwrap();
    divergent = divergent.replace(
        "Keep all current planning",
        "Changed only in linked worktree",
    );
    fs::write(&work_path, divergent).unwrap();
    let root_before = fixture.source_snapshot();
    let worktree_before = fs::read(&work_path).unwrap();

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning sources contain divergent")
    );
    let conflict_path = fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("conflicts"))
        .expect("structured conflict manifest");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict_path).unwrap()).unwrap();
    assert!(
        report["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| {
                conflict["recordId"] == fixture.work_uid
                    && conflict["sources"].as_array().is_some_and(|sources| {
                        sources.iter().any(|source| {
                            source["disposition"] == "linked-worktree-snapshot"
                                && source["sourceRef"] == "feature/divergent"
                        })
                    })
            })
    );
    assert_eq!(fixture.source_snapshot(), root_before);
    assert_eq!(fs::read(work_path).unwrap(), worktree_before);
}
