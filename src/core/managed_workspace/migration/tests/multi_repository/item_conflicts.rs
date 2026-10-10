use std::collections::BTreeMap;

use super::super::support::Fixture;
use super::helpers::{create_secondary, planning_repo_count, register_alpha};
use crate::artifacts::{
    items_io,
    planning_store::{PlanningStore, paths},
};
use sha2::Digest as _;

#[test]
fn open_resolved_divergence_across_sources_has_a_structured_preflight_conflict() {
    let fixture = Fixture::new("multi-repository-item-state-conflict");
    register_alpha(&fixture);
    let alpha = create_secondary(&fixture);
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &alpha);
    let (mut open, mut resolved, _) = items_io::load_store(&store).unwrap();
    let mut moved = open.remove(0);
    moved.status = crate::domain::ItemStatus::Resolved;
    resolved.push(moved);
    store
        .atomic_write(paths::OPEN_ITEMS, items_io::serialize(&open).as_bytes())
        .unwrap();
    store
        .atomic_write(
            paths::RESOLVED_ITEMS,
            &serde_json::to_vec_pretty(&resolved).unwrap(),
        )
        .unwrap();
    super::super::support::git(&alpha, &["add", ".koolade-packet"]);
    super::super::support::git(
        &alpha,
        &["commit", "--quiet", "-m", "resolve item in alpha source"],
    );

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
    assert_eq!(planning_repo_count(&fixture), 0);
    let conflict_path = std::fs::read_dir(fixture.root.join("operator/projects/.migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("conflicts"))
        .expect("structured conflict manifest");
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(conflict_path).unwrap()).unwrap();
    let conflict = report["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|conflict| conflict["kind"] == "item-state-divergence")
        .expect("item state conflict");
    assert_eq!(
        conflict["recordId"],
        fixture.open_item.uid.as_deref().unwrap()
    );
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
    let root = sources
        .iter()
        .find(|source| source["repositoryId"] == "root")
        .unwrap();
    let alpha_artifact = sources
        .iter()
        .find(|source| source["repositoryId"] == "alpha")
        .unwrap();
    assert_eq!(root["sourcePath"], ".koolade-packet/planning/open-items.md");
    assert_eq!(
        alpha_artifact["sourcePath"],
        ".koolade-packet/planning/resolved-items.json"
    );
    assert_eq!(root["disposition"], "record-source-open");
    assert_eq!(alpha_artifact["disposition"], "record-source-resolved");
    assert_eq!(
        root["sha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(
                std::fs::read(fixture.code.join(".koolade-packet/planning/open-items.md")).unwrap()
            )
        )
    );
    assert_eq!(
        alpha_artifact["sha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(
                std::fs::read(alpha.join(".koolade-packet/planning/resolved-items.json")).unwrap()
            )
        )
    );
}
