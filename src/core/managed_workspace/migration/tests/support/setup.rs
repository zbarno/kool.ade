use std::fs;

use super::fixtures::{feature_markdown, identity_markdown};
use super::{git, multi_feature};
use crate::{
    artifacts::{
        items_io,
        planning_store::{PlanningStore, paths},
    },
    core::{
        managed_workspace::WorkspaceManager,
        planning_work::{Work, WorkKind, WorkStatus},
    },
    domain::{
        ArtifactIdentity, Authority, ChangeMetadata, ChangeStatus, ItemKind, ItemStatus, OpenItem,
        Priority,
    },
};

pub(super) fn build(label: &str, include_workspace_id: bool) -> super::Fixture {
    let root = std::env::temp_dir().join(format!(
        "koolade-migration-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    let code = root.join("code");
    fs::create_dir_all(&code).unwrap();
    git(&code, &["init", "--quiet", "--initial-branch=main"]);
    git(&code, &["config", "user.name", "Koolade Test"]);
    git(
        &code,
        &["config", "user.email", "koolade-test@example.invalid"],
    );
    let manager = WorkspaceManager::new(root.join("operator"));
    let legacy_workspace_id = include_workspace_id.then(uuid::Uuid::new_v4);
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &code);
    crate::artifacts::migration::bootstrap_product_with_store(&code, &store, "Legacy workspace")
        .unwrap();
    {
        let manifest_path = code.join(".koolade-packet/manifest.json");
        let mut manifest = if manifest_path.is_file() {
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap()
        } else {
            serde_json::json!({"schemaVersion": 2, "product": "Legacy workspace"})
        };
        if let Some(id) = legacy_workspace_id {
            manifest["projectId"] = serde_json::Value::String(id.hyphenated().to_string());
        }
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }
    let mut state = crate::core::state::PlannerState::load_with_store(&code, &store).unwrap();
    state.bootstrap_missing().unwrap();

    let feature = ArtifactIdentity::new("F1", "Invoice batching");
    let feature_text = feature_markdown(&feature);
    store
        .atomic_write(
            "planning/changes/F1-invoice-batching/specification.md",
            feature_text.as_bytes(),
        )
        .unwrap();
    let second = multi_feature::add(&store);
    store
        .atomic_write(
            "planning/changes/F1-invoice-batching/metadata.json",
            &serde_json::to_vec_pretty(&ChangeMetadata::new(&feature, ChangeStatus::Ready))
                .unwrap(),
        )
        .unwrap();
    let batch = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "BATCH-001".into(),
        title: "Invoice batch tasks".into(),
        parent_uid: Some(feature.uid.clone()),
    };
    let task = ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "TASK-001".into(),
        title: "Add batch endpoint".into(),
        parent_uid: Some(batch.uid.clone()),
    };
    let metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&task, "root", Vec::new()).unwrap();
    let task_md = format!(
        "---\nkoolade-task: {}\n---\n\n{}",
        serde_json::to_string(&metadata).unwrap(),
        identity_markdown(
            &task,
            "# TASK-001: Add batch endpoint\n\nImplement the endpoint.\n",
        )
    );
    store
        .atomic_write(
            "planning/tasks/F1-invoice-batching/README.md",
            identity_markdown(&batch, "# Invoice batch tasks\n").as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/tasks/F1-invoice-batching/specification.md",
            feature_text.as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/tasks/F1-invoice-batching/001-add-batch-endpoint.md",
            task_md.as_bytes(),
        )
        .unwrap();

    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let work = Work {
        uid: work_uid.clone(),
        key: "MIG-001".into(),
        kind: WorkKind::Feature,
        title: "Migrate invoice planning".into(),
        request: "Keep all current planning".into(),
        status: WorkStatus::NeedsAttention,
        feature_id: Some("F1".into()),
        feature_uid: Some(feature.uid.clone()),
        parent_uid: None,
        source_branch: Some("feature/invoice".into()),
        destination_branch: Some("main".into()),
        routing_overrides: Default::default(),
        routing_inherited_from: None,
        follow_up_task: None,
        detail: "Preserve the approval and task state.".into(),
        record_revision: 0,
        record_baseline: None,
    };
    store
        .atomic_write(
            paths::WORK,
            &serde_json::to_vec_pretty(&serde_json::json!({"schemaVersion":3,"items":[work]}))
                .unwrap(),
        )
        .unwrap();

    let mut open_item = OpenItem::new(
        "CLR-001".into(),
        Priority::High,
        ItemKind::Question,
        "Product".into(),
        None,
        "Should retries use the same key?".into(),
        "The endpoint needs retry semantics.".into(),
    );
    open_item.uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    open_item.authority = Authority::Human;
    open_item.feature_id = Some("F1".into());
    let mut resolved_item = OpenItem::new(
        "CLR-002".into(),
        Priority::Normal,
        ItemKind::Ambiguity,
        "Product".into(),
        None,
        "Is batch order stable?".into(),
        "Consumers need deterministic results.".into(),
    );
    resolved_item.uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    resolved_item.status = ItemStatus::Resolved;
    resolved_item.recommendation = "Use creation order.".into();
    store
        .atomic_write(
            paths::OPEN_ITEMS,
            items_io::serialize(std::slice::from_ref(&open_item)).as_bytes(),
        )
        .unwrap();
    store
        .atomic_write(
            paths::RESOLVED_ITEMS,
            &serde_json::to_vec_pretty(&vec![resolved_item.clone()]).unwrap(),
        )
        .unwrap();

    let workflow = crate::core::workflow::Workflow {
        approved_features: [
            ("F1".into(), format!("sha256:{}", "a".repeat(64))),
            ("F2".into(), format!("sha256:{}", "b".repeat(64))),
        ]
        .into(),
        feature_branch_targets: [
            (
                "F1".into(),
                crate::core::workflow::BranchTargets {
                    source: "feature/invoice".into(),
                    destination: "main".into(),
                },
            ),
            (
                "F2".into(),
                crate::core::workflow::BranchTargets {
                    source: "feature/refund".into(),
                    destination: "release/1.2".into(),
                },
            ),
        ]
        .into(),
        task_batches: vec![
            crate::core::workflow::TaskBatchRef {
                identity: Some(batch.clone()),
                feature: "Invoice batching".into(),
                directory: "planning/tasks/F1-invoice-batching".into(),
                count: 1,
                created_at_ms: 17,
            },
            crate::core::workflow::TaskBatchRef {
                identity: Some(second.batch.clone()),
                feature: "Refund export".into(),
                directory: "planning/tasks/F2-refund-export".into(),
                count: 1,
                created_at_ms: 19,
            },
        ],
        ..Default::default()
    };
    store
        .atomic_write(
            paths::WORKFLOW,
            &serde_json::to_vec_pretty(&workflow).unwrap(),
        )
        .unwrap();
    store.atomic_write(
            format!("{}/{}.json", paths::TASK_STATES, task.uid),
            &serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion":1,"uid":task.uid,"revision":2,"createdAtMs":17,"updatedAtMs":29,
                "data":{"batchUid":batch.uid,"repositoryId":"root","dependencyUids":[],"status":"in_progress","executionStatus":"implementing"}
            })).unwrap(),
        ).unwrap();
    store
            .atomic_write(
                format!("{}/{}.json", paths::TASK_STATES, second.task.uid),
                &serde_json::to_vec_pretty(&serde_json::json!({
                    "schemaVersion":1,"uid":second.task.uid,"revision":1,"createdAtMs":19,"updatedAtMs":23,
                    "data":{"batchUid":second.batch.uid,"repositoryId":"root","dependencyUids":[],"status":"todo","executionStatus":"not_started"}
                })).unwrap(),
            )
            .unwrap();
    store
        .atomic_write(
            "planning/archive/specification-pre-modules.md",
            b"Archived specification evidence\n",
        )
        .unwrap();
    fs::write(
        code.join(".koolade-packet/config/mcp.json"),
        b"{\"mcpServers\":{\"fixture\":{}}}",
    )
    .unwrap();
    fs::create_dir_all(code.join(".koolade-packet/implementation")).unwrap();
    fs::write(
        code.join(".koolade-packet/implementation/evidence.json"),
        b"{\"synthetic\":true}",
    )
    .unwrap();
    fs::write(
        code.join(".koolade-packet/state/time-ledger.log"),
        b"private ledger fixture\n",
    )
    .unwrap();
    fs::write(
        code.join(".koolade-packet/config/migration-secret.txt"),
        b"private unknown config\n",
    )
    .unwrap();
    git(&code, &["add", "."]);
    git(
        &code,
        &["commit", "--quiet", "-m", "legacy planning fixture"],
    );
    super::Fixture {
        root,
        code,
        manager,
        feature,
        feature_two: second.feature,
        task,
        work_uid,
        open_item,
        resolved_item,
        task_two: second.task,
        legacy_workspace_id,
    }
}
