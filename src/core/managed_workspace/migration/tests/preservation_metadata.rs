use std::fs;

use super::support::Fixture;

#[test]
fn migration_reuses_existing_workspace_association_without_allocating_a_second_id() {
    let fixture = Fixture::new("repeat");
    let first = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();
    let second = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.planning_repo, second.planning_repo);
}

#[test]
fn legacy_manifest_workspace_id_and_project_id_must_agree() {
    let fixture = Fixture::new("workspace-id-alias-conflict");
    let manifest_path = fixture.code.join(".koolade-packet/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["workspaceId"] =
        serde_json::Value::String(uuid::Uuid::new_v4().hyphenated().to_string());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("workspaceId and projectId disagree")
    );
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unsupported_legacy_manifest_schema_stops_before_cutover() {
    let fixture = Fixture::new("unsupported-legacy-manifest");
    let manifest_path = fixture.code.join(".koolade-packet/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["schemaVersion"] = 5.into();
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("unsupported legacy planning manifest schema 5")
    );
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
}

#[test]
fn rollback_warns_when_the_managed_planning_repo_has_new_changes() {
    let fixture = Fixture::new("rollback-with-changes");
    let workspace = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();
    fs::write(
        workspace.planning_repo.join("operator-note.md"),
        "New planning work needs export before rollback.\n",
    )
    .unwrap();

    let rollback = fixture
        .manager
        .rollback_legacy_migration(&fixture.code)
        .unwrap();

    assert!(rollback.detached);
    assert!(
        rollback
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("Export them"))
    );
}
