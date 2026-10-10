use std::{collections::BTreeMap, fs};

use super::support::Fixture;
use helpers::{
    append_unique_work, change_existing_work, change_feature_approval, create_secondary,
    planning_repo_count, register_alpha, snapshot,
};

#[path = "multi_repository/helpers.rs"]
mod helpers;
use crate::core::managed_workspace::migration::MigrationReport;

#[path = "multi_repository/item_conflicts.rs"]
mod item_conflicts;

#[test]
fn registered_code_repositories_aggregate_into_one_planning_workspace() {
    let fixture = Fixture::new("multi-repository");
    register_alpha(&fixture);
    let alpha = create_secondary(&fixture);
    append_unique_work(&alpha);
    let root_before = fixture.source_snapshot();
    let alpha_before = snapshot(&alpha.join(".koolade-packet"));
    let sources = BTreeMap::from([("alpha".into(), alpha.clone())]);

    let workspace = fixture
        .manager
        .migrate_legacy_checkout_with_registered_sources(&fixture.code, &sources)
        .unwrap();

    let work = crate::core::planning_work::load(&workspace.store).unwrap();
    assert_eq!(work.len(), 2);
    assert!(work.iter().any(|record| record.uid == fixture.work_uid));
    assert!(work.iter().any(|record| record.key == "MIG-002"));
    assert_eq!(workspace.association.code_checkouts.len(), 2);
    assert_eq!(
        fixture
            .manager
            .workspace_for_checkout(&alpha)
            .unwrap()
            .unwrap()
            .id,
        workspace.id
    );
    let report: MigrationReport = serde_json::from_slice(
        &fs::read(workspace.planning_repo.join("migration-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report.source_repositories.len(), 2);
    assert!(
        report
            .source_repositories
            .iter()
            .all(|source| source.available)
    );
    assert!(report.source_artifacts.iter().any(|artifact| {
        artifact.repository_id.as_deref() == Some("alpha")
            && artifact.source_path == ".koolade-packet/state/work.json"
    }));
    let board = crate::artifacts::task_docs::load_board(
        &workspace.store,
        &crate::artifacts::task_docs::load_workflow(&workspace.store).unwrap(),
    );
    assert_eq!(
        board
            .iter()
            .filter(|task| task
                .identity
                .as_ref()
                .is_some_and(|identity| identity.uid == fixture.task.uid))
            .count(),
        1
    );
    assert_eq!(fixture.source_snapshot(), root_before);
    assert_eq!(snapshot(&alpha.join(".koolade-packet")), alpha_before);
    assert_eq!(planning_repo_count(&fixture), 1);
}

#[test]
fn missing_registered_checkout_is_reported_and_does_not_block_planning_migration() {
    let fixture = Fixture::new("missing-registered-repository");
    register_alpha(&fixture);
    let missing = fixture.root.join("offline-alpha");
    let sources = BTreeMap::from([("alpha".into(), missing)]);

    let workspace = fixture
        .manager
        .migrate_legacy_checkout_with_registered_sources(&fixture.code, &sources)
        .unwrap();

    assert_eq!(workspace.repositories.repositories.len(), 2);
    assert_eq!(workspace.association.code_checkouts.len(), 1);
    let report: MigrationReport = serde_json::from_slice(
        &fs::read(workspace.planning_repo.join("migration-report.json")).unwrap(),
    )
    .unwrap();
    let alpha = report
        .source_repositories
        .iter()
        .find(|source| source.repository_id == "alpha")
        .unwrap();
    assert!(!alpha.available);
    assert_eq!(alpha.artifact_count, 0);
    assert!(
        crate::core::planning_work::load(&workspace.store)
            .unwrap()
            .iter()
            .any(|work| work.uid == fixture.work_uid)
    );
}

#[test]
fn conflicting_records_from_registered_checkouts_emit_source_hashes_and_abort() {
    let fixture = Fixture::new("multi-repository-conflict");
    register_alpha(&fixture);
    let alpha = create_secondary(&fixture);
    change_existing_work(&alpha);
    let sources = BTreeMap::from([("alpha".into(), alpha.clone())]);

    let error = fixture
        .manager
        .migrate_legacy_checkout_with_registered_sources(&fixture.code, &sources)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning sources contain divergent")
    );
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
    let conflict_path = fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("conflicts"))
        .expect("structured conflict manifest");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict_path).unwrap()).unwrap();
    let conflict = report["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|conflict| conflict["kind"] == "work-record-divergence")
        .expect("work record conflict");
    assert_eq!(conflict["recordId"], fixture.work_uid);
    let sources = conflict["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(
        sources
            .iter()
            .map(|source| source["repositoryId"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["alpha", "root"])
    );
    assert_ne!(sources[0]["sha256"], sources[1]["sha256"]);
    assert!(sources.iter().all(|source| {
        source["sha256"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
    }));
    assert_eq!(planning_repo_count(&fixture), 0);
}

#[test]
fn different_legacy_workspace_ids_are_reported_instead_of_merged() {
    let fixture = Fixture::new("workspace-identity-conflict");
    register_alpha(&fixture);
    let alpha = create_secondary(&fixture);
    let manifest_path = alpha.join(".koolade-packet/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["projectId"] =
        serde_json::Value::String(uuid::Uuid::new_v4().hyphenated().to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    super::support::git(&alpha, &["add", ".koolade-packet/manifest.json"]);
    super::support::git(
        &alpha,
        &[
            "commit",
            "--quiet",
            "-m",
            "change legacy workspace identity",
        ],
    );
    let sources = BTreeMap::from([("alpha".into(), alpha)]);

    let error = fixture
        .manager
        .migrate_legacy_checkout_with_registered_sources(&fixture.code, &sources)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("planning sources contain divergent")
    );
    let conflicts = fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&fs::read(conflicts).unwrap()).unwrap();
    let identity = report["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|conflict| conflict["kind"] == "workspace-identity-divergence")
        .unwrap();
    let sources = identity["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|source| {
        source["sha256"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
    }));
    assert_eq!(planning_repo_count(&fixture), 0);
}

#[test]
fn approval_fingerprint_conflicts_name_the_stable_feature_uid() {
    let fixture = Fixture::new("approval-fingerprint-conflict");
    register_alpha(&fixture);
    let alpha = create_secondary(&fixture);
    change_feature_approval(&alpha);
    let sources = BTreeMap::from([("alpha".into(), alpha)]);

    let error = fixture
        .manager
        .migrate_legacy_checkout_with_registered_sources(&fixture.code, &sources)
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
        .unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(conflict_path).unwrap()).unwrap();
    let conflict = report["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|conflict| conflict["kind"] == "approval-divergence")
        .unwrap();
    assert_eq!(conflict["recordId"], fixture.feature.uid);
    assert_eq!(conflict["sources"].as_array().unwrap().len(), 2);
    assert_eq!(planning_repo_count(&fixture), 0);
}
