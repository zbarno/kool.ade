use crate::{
    artifacts::{migration, planning_store::PlanningStore},
    core::{
        implementation::Implementation,
        lifecycle::{
            FeatureGoal, FeaturePhase, LifecycleStore, PullRequestPreparation,
            PullRequestPreparationStatus, SpecReconciliationStatusV1, SpecRevisionDraftV1,
            TaskRecord,
        },
    },
};
use sha2::Digest as _;

mod task;

pub(super) fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(in crate::core::implementation) struct Fixture {
    _owned_root: Option<tempfile::TempDir>,
    repository_root: std::path::PathBuf,
    store_root: std::path::PathBuf,
    pub store: PlanningStore,
    pub work_uid: String,
    pub reviewed_head: String,
    pub task: TaskRecord,
}

impl Fixture {
    pub fn new(oid_length: usize) -> Self {
        Self::with_reviewed_head(oid_length, &"a".repeat(oid_length), id())
    }

    pub fn with_reviewed_head(
        oid_length: usize,
        reviewed_head: &str,
        planning_item_uid: String,
    ) -> Self {
        Self::build(oid_length, reviewed_head, planning_item_uid, false)
    }

    fn build(
        oid_length: usize,
        reviewed_head: &str,
        planning_item_uid: String,
        include_subtask: bool,
    ) -> Self {
        assert!(matches!(reviewed_head.len(), 40 | 64));
        assert_eq!(reviewed_head.len(), oid_length);
        let root = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        migration::bootstrap_product(root.path(), "Publication Fixture").unwrap();
        let store = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), root.path());
        let repository_root = store.root.parent().unwrap().to_path_buf();
        Self::build_records(
            Some(root),
            store,
            repository_root,
            id(),
            reviewed_head,
            planning_item_uid,
            include_subtask,
        )
    }

    pub(in crate::core::implementation) fn with_store(
        store: PlanningStore,
        repository_root: std::path::PathBuf,
        feature_uid: String,
        reviewed_head: &str,
        planning_item_uid: String,
        include_subtask: bool,
    ) -> Self {
        assert!(matches!(reviewed_head.len(), 40 | 64));
        Self::build_records(
            None,
            store,
            repository_root,
            feature_uid,
            reviewed_head,
            planning_item_uid,
            include_subtask,
        )
    }

    fn build_records(
        owned_root: Option<tempfile::TempDir>,
        store: PlanningStore,
        repository_root: std::path::PathBuf,
        feature_uid: String,
        reviewed_head: &str,
        planning_item_uid: String,
        include_subtask: bool,
    ) -> Self {
        let store_root = store.root.clone();
        let lifecycle = LifecycleStore::new(&store);
        let origin_uid = id();
        lifecycle
            .create_feature(
                &FeatureGoal {
                    schema_version: 1,
                    feature_uid: feature_uid.clone(),
                    original_intent: "Publish only reviewed work".into(),
                    draft: String::new(),
                    scope: Vec::new(),
                    non_goals: Vec::new(),
                    revision: 0,
                    phase: FeaturePhase::Exploring,
                    approved_plan_sha256: None,
                    plan_snapshot_uid: None,
                    plan_snapshot_sha256: None,
                    unresolved_questions: Vec::new(),
                    decision_refs: Vec::new(),
                    review_refs: Vec::new(),
                    readiness_evidence_refs: Vec::new(),
                    task_uids: Vec::new(),
                },
                &origin_uid,
            )
            .unwrap();
        let product = crate::artifacts::product_docs::render_product(&store)
            .unwrap()
            .unwrap();
        lifecycle
            .append_spec_revision(
                &feature_uid,
                0,
                SpecRevisionDraftV1 {
                    content_hash: format!(
                        "{:x}",
                        sha2::Sha256::digest(serde_json::to_vec(&Some(product)).unwrap())
                    ),
                    source_revision: 1,
                    produced_by_run_id: None,
                    origin_events: vec![origin_uid],
                    decision_refs: Vec::new(),
                    delta_approval_refs: Vec::new(),
                    approved_target_hash: None,
                    implementation_head_sha: None,
                    reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                    outcome_reason: None,
                    no_change_reason: Some(
                        "Synthetic fixture records an explicit unchanged-specification result."
                            .into(),
                    ),
                    deltas: Vec::new(),
                },
            )
            .unwrap();
        let work_uid = id();
        // Validated synthetic prior-stage evidence: this fixture does not claim
        // to run independent review or interactive runtime QA itself.
        let mut task = task::completed(&feature_uid, &work_uid, reviewed_head);
        task.planning_item_uid = Some(planning_item_uid);
        let child = include_subtask.then(|| {
            let mut child = task::completed(&feature_uid, &work_uid, reviewed_head);
            child.parent_task_uid = Some(task.task_uid.clone());
            child.planning_item_uid = None;
            child.pr_work_item_uid = None;
            task.subtask_uids.push(child.task_uid.clone());
            child.validate().unwrap();
            child
        });
        task.validate().unwrap();
        let mut included_task_uids = vec![task.task_uid.clone()];
        if let Some(child) = &child {
            included_task_uids.push(child.task_uid.clone());
        }
        let work = PullRequestPreparation {
            schema_version: 1,
            uid: work_uid.clone(),
            work_item_uid: work_uid.clone(),
            feature_uid,
            task_uid: task.task_uid.clone(),
            included_task_uids,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            status: PullRequestPreparationStatus::AwaitingAuthorization,
            publication_authorization: None,
            recovery: None,
            provider_pr_id: None,
            provider_pr_url: None,
            head_sha: None,
            base_sha: None,
            merged_commit_sha: None,
        };
        let mut records = vec![
            snapshot("lifecycle_tasks", &task.task_uid, task.revision, &task),
            snapshot("lifecycle_pr_work_items", &work_uid, 1, &work),
        ];
        if let Some(child) = &child {
            records.push(snapshot(
                "lifecycle_tasks",
                &child.task_uid,
                child.revision,
                child,
            ));
        }
        store.transaction(&records, None).unwrap();
        Self {
            _owned_root: owned_root,
            repository_root,
            store_root,
            store,
            work_uid,
            reviewed_head: reviewed_head.to_owned(),
            task,
        }
    }

    pub fn reopen(&self) -> PlanningStore {
        PlanningStore::new(
            self.store.project_id,
            self.store_root.clone(),
            self.store.mode,
        )
    }

    pub fn implementation(&self) -> Implementation {
        serde_json::from_value(serde_json::json!({
            "ticket": "TASK-001.md", "task_uid": self.task.planning_item_uid,
            "ticket_text": "# Synthetic publication fixture", "branch": "task/fixture",
            "base": "main", "base_commit": "d".repeat(40),
            "task_repository": self.repository_root, "status": "awaiting_approval", "detail": "",
            "pr_url": null, "verified_head": self.reviewed_head,
        }))
        .unwrap()
    }
}

fn snapshot<T: serde::Serialize>(
    directory: &str,
    uid: &str,
    revision: u64,
    record: &T,
) -> (String, Vec<u8>) {
    (
        format!("state/{directory}/{uid}.json"),
        serde_json::to_vec(&serde_json::json!({
            "uid": uid, "schemaVersion": 1, "createdAtMs": 1, "updatedAtMs": 1,
            "revision": revision, "record": record,
        }))
        .unwrap(),
    )
}
