use super::support::Fixture;
use crate::{
    artifacts::{items_io, planning_store::paths},
    core::planning_work::WorkStatus,
    domain::ItemStatus,
};
use std::fs;

#[test]
fn migration_preserves_shared_state_and_keeps_private_data_out_of_git() {
    let fixture = Fixture::new("preserves");
    let source_before = fixture.source_snapshot();
    let source_status = super::support::git(&fixture.code, &["status", "--porcelain"]);
    let workspace = fixture
        .manager
        .migrate_legacy_checkout(&fixture.code)
        .unwrap();

    assert_eq!(workspace.store.project_id, workspace.manifest.workspace_id);
    assert_eq!(workspace.id, fixture.legacy_workspace_id.unwrap());
    assert_eq!(workspace.association.code_checkouts.len(), 1);
    assert_eq!(
        workspace.association.code_checkouts[0].repository_id,
        "root"
    );
    assert_eq!(workspace.association.code_checkouts[0].path, fixture.code);
    assert!(!workspace.planning_repo.join(".koolade-packet").exists());
    assert!(!workspace.planning_repo.join(paths::WORK).exists());
    assert!(!workspace.planning_repo.join(paths::WORKFLOW).exists());
    assert!(!workspace.planning_repo.join(paths::OPEN_ITEMS).exists());
    assert!(!workspace.planning_repo.join(paths::RESOLVED_ITEMS).exists());

    let work = crate::core::planning_work::load(&workspace.store).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].uid, fixture.work_uid);
    assert_eq!(work[0].status, WorkStatus::NeedsAttention);
    assert_eq!(
        work[0].feature_uid.as_deref(),
        Some(fixture.feature.uid.as_str())
    );
    assert_eq!(work[0].source_branch.as_deref(), Some("feature/invoice"));

    let workflow = crate::artifacts::task_docs::load_workflow(&workspace.store).unwrap();
    assert_eq!(
        workflow.approved_features["F1"],
        format!("sha256:{}", "a".repeat(64))
    );
    assert_eq!(
        workflow.approved_features["F2"],
        format!("sha256:{}", "b".repeat(64))
    );
    assert_eq!(workflow.feature_record_ids["F2"], fixture.feature_two.uid);
    assert_eq!(workflow.feature_branch_targets["F1"].destination, "main");
    assert_eq!(
        workflow.feature_branch_targets["F2"].destination,
        "release/1.2"
    );
    assert_eq!(workflow.task_batches.len(), 2);
    assert!(
        workspace
            .planning_repo
            .join(format!(
                "{}/{}.json",
                paths::WORKFLOW_RECORDS,
                fixture.feature_two.uid
            ))
            .is_file()
    );
    assert!(
        workspace
            .planning_repo
            .join(format!(
                "{}/{}.json",
                paths::WORKFLOW_RECORDS,
                workflow.feature_record_ids["F1"]
            ))
            .is_file()
    );

    let (open, resolved, _) = items_io::load_store(&workspace.store).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(resolved.len(), 1);
    assert_eq!(open[0].uid, fixture.open_item.uid);
    assert_eq!(resolved[0].uid, fixture.resolved_item.uid);
    assert_eq!(open[0].id, "CLR-001");
    assert_eq!(resolved[0].status, ItemStatus::Resolved);
    assert!(
        workspace
            .planning_repo
            .join(format!(
                "{}/{}.md",
                paths::ITEM_CONTENTS,
                fixture.open_item.uid.as_deref().unwrap()
            ))
            .is_file()
    );

    let task_state: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.planning_repo.join(format!(
            "{}/{}.json",
            paths::TASK_STATES,
            fixture.task.uid
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(task_state["revision"], 2);
    assert_eq!(task_state["data"]["status"], "in_progress");
    assert_eq!(task_state["data"]["executionStatus"], "implementing");
    assert!(
        crate::artifacts::task_docs::load_board(&workspace.store, &workflow)
            .iter()
            .any(|task| task
                .identity
                .as_ref()
                .is_some_and(|identity| identity.uid == fixture.task.uid))
    );
    let board = crate::artifacts::task_docs::load_board(&workspace.store, &workflow);
    assert!(board.iter().any(|task| {
        task.identity
            .as_ref()
            .is_some_and(|identity| identity.uid == fixture.task_two.uid)
    }));
    let second_task_state: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.planning_repo.join(format!(
            "{}/{}.json",
            paths::TASK_STATES,
            fixture.task_two.uid
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(second_task_state["data"]["status"], "todo");

    assert!(
        workspace
            .planning_repo
            .join("planning/archive/specification-pre-modules.md")
            .is_file()
    );
    let private = fixture.manager.project_dir(workspace.id).join("local");
    assert_eq!(
        fs::read(private.join("config/mcp.json")).unwrap(),
        b"{\"mcpServers\":{\"fixture\":{}}}"
    );
    assert_eq!(
        fs::read(private.join("implementation/evidence.json")).unwrap(),
        b"{\"synthetic\":true}"
    );
    assert_eq!(
        fs::read(private.join("state/time-ledger.log")).unwrap(),
        b"private ledger fixture\n"
    );
    assert!(!workspace.planning_repo.join("config/mcp.json").exists());
    assert!(!workspace.planning_repo.join("implementation").exists());

    let report: crate::core::managed_workspace::MigrationReport = serde_json::from_slice(
        &fs::read(workspace.planning_repo.join("migration-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report.workspace_id, workspace.id);
    assert!(report.source_artifacts.iter().any(|entry| entry.source_path
        == ".koolade-packet/state/work.json"
        && entry.disposition == "normalized-into-per-record-layout"));
    assert!(
        report
            .source_artifacts
            .iter()
            .any(|entry| entry.source_path == ".koolade-packet/manifest.json"
                && entry.target_path.as_deref() == Some("manifest.json")
                && entry.disposition == "replaced-by-workspace-manifest")
    );
    assert!(report.source_artifacts.iter().any(|entry| entry.source_path
        == ".koolade-packet/config/mcp.json"
        && entry.target_path.as_deref() == Some("operator-local/config/mcp.json")
        && entry.sha256.is_none()));
    assert!(report.source_artifacts.iter().any(|entry| entry.source_path
        == ".koolade-packet/config/migration-secret.txt"
        && entry.disposition == "copied-to-operator-local-storage"));
    let public_report =
        fs::read_to_string(workspace.planning_repo.join("migration-report.json")).unwrap();
    assert!(!public_report.contains(&fixture.root.to_string_lossy().to_string()));

    assert_eq!(fixture.source_snapshot(), source_before);
    assert_eq!(
        super::support::git(&fixture.code, &["status", "--porcelain"]),
        source_status
    );
    assert_eq!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .unwrap()
            .id,
        workspace.id
    );

    let rollback = fixture
        .manager
        .rollback_legacy_migration(&fixture.code)
        .unwrap();
    assert!(rollback.detached);
    assert!(rollback.warning.is_none());
    assert!(
        fixture
            .manager
            .workspace_for_checkout(&fixture.code)
            .unwrap()
            .is_none()
    );
}
