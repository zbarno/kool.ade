use crate::{
    artifacts::planning_store::PlanningStore,
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

mod published_specs;
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
        let repository_root = root.path().join("code");
        std::fs::create_dir_all(&repository_root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&repository_root)
                .status()
                .unwrap()
                .success()
        );
        let product_root = repository_root.join("docs/koolade/product");
        std::fs::create_dir_all(&product_root).unwrap();
        std::fs::write(
            product_root.join("overview.md"),
            "# Publication Fixture\n\nA synthetic published product for lifecycle tests.\n",
        )
        .unwrap();
        std::fs::write(
            product_root.join("index.md"),
            "# Product\n\n- [Overview](./overview.md)\n",
        )
        .unwrap();
        std::fs::write(
            repository_root.join("docs/koolade/AGENTS.md"),
            "# Navigation\n\n- [Product index](./product/index.md) - Published product documentation.\n",
        )
        .unwrap();
        std::fs::write(
            product_root.join("AGENTS.md"),
            "# Navigation\n\n- [Overview](./overview.md) - Product overview.\n- [index.md](./index.md) - Product document index.\n",
        )
        .unwrap();
        for args in [
            ["config", "user.name", "Publication Fixture"].as_slice(),
            [
                "config",
                "user.email",
                "publication-fixture@example.invalid",
            ]
            .as_slice(),
            ["add", "docs/koolade"].as_slice(),
            [
                "commit",
                "--quiet",
                "-m",
                "Publish synthetic lifecycle specification",
            ]
            .as_slice(),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&repository_root)
                    .status()
                    .unwrap()
                    .success(),
                "could not seed published-spec fixture commit"
            );
        }
        let project_id = uuid::Uuid::new_v4();
        let mut store = PlanningStore::for_code_repository(
            project_id,
            root.path().join("private"),
            &repository_root,
        );
        store.published_spec_pin = Some(
            crate::core::published_specs::PublishedSpecReader::for_primary_repository(
                &repository_root,
            )
            .unwrap()
            .pin()
            .unwrap(),
        );
        let repositories = crate::core::project_repos::ProjectManifest {
            repositories: vec![crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Primary code".into(),
                remote: String::new(),
                display_name: None,
            }],
        };
        store
            .atomic_write(
                crate::artifacts::layout::store::PROJECT_MANIFEST,
                &serde_json::to_vec(&repositories).unwrap(),
            )
            .unwrap();
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
        published_specs::seed_if_missing(&repository_root).unwrap();
        let store =
            PlanningStore::for_code_repository(store.project_id, &store.root, &repository_root);
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
        approve_fixture_feature(&store, &lifecycle, &feature_uid);
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
            publication_scope: None,
            lease_epoch: 0,
            publication_authorization_event_uids: Vec::new(),
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
        PlanningStore::for_code_repository(
            self.store.project_id,
            &self.store_root,
            &self.repository_root,
        )
    }

    pub fn implementation(&self) -> Implementation {
        serde_json::from_value(serde_json::json!({
            "ticket": "TASK-001.md", "task_uid": self.task.planning_item_uid,
            "ticket_text": "# Synthetic publication fixture", "branch": "task/fixture",
            "base": "main", "base_commit": "d".repeat(40),
            "repository_id": "fixture-repository", "destination_branch": "main",
            "task_repository": self.repository_root, "task_repository_kind": "clone", "status": "awaiting_approval", "detail": "",
            "pr_url": null, "verified_head": self.reviewed_head,
        }))
        .unwrap()
    }

    pub fn bind_task_contents(&mut self, task_contents: &str) {
        let approved_plan = self.task.approved_plan_sha256.as_str();
        self.task.task_spec_sha256 = Some(format!(
            "{:x}",
            sha2::Sha256::digest(
                serde_json::to_vec(&(1_u16, approved_plan, task_contents)).unwrap()
            )
        ));
        self.task.validate().unwrap();
        self.store
            .transaction(
                &[snapshot(
                    "lifecycle_tasks",
                    &self.task.task_uid,
                    self.task.revision,
                    &self.task,
                )],
                None,
            )
            .unwrap();
    }
}

fn approve_fixture_feature(
    store: &PlanningStore,
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
) {
    use crate::core::lifecycle::{FeatureEvent, GateActor, GateDecision, GateReceipt};
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};

    let feature = lifecycle.load_feature(feature_uid).unwrap();
    let feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["fixture:approved-feature-readiness/v1".into()],
            },
        )
        .unwrap();
    let plan_sha256 = feature.plan_sha256();
    let feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_sha256.clone(),
            },
        )
        .unwrap();
    let user_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: feature_uid.into(),
        subject_sha256: plan_sha256.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:explicit-user-approval/v1".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::UserPlanDecision(user_receipt),
        )
        .unwrap();
    let role = PromptRole::PlanReviewer;
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: id(),
        feature_uid: feature_uid.into(),
        task_uid: None,
        original_goal: "Approve a synthetic publication fixture".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "fixture:publication/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: feature.revision.to_string(),
            subject_hash: plan_sha256.clone(),
            author_run_id: id(),
            reviewer_run_id: id(),
            author_session_id: Some(id()),
            reviewer_session_id: Some(id()),
            requirements_matrix: Vec::new(),
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    let review_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: feature_uid.into(),
        subject_sha256: plan_sha256.clone(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:plan-review/v1".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(id()),
        author_run_id: Some(id()),
        reviewer_session_id: Some(id()),
        author_session_id: Some(id()),
    };
    let feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::PlanReview(review_receipt),
        )
        .unwrap();
    let feature = lifecycle
        .apply_feature_event(
            feature_uid,
            feature.revision,
            &id(),
            FeatureEvent::TaskGraphCreated { plan_sha256 },
        )
        .unwrap();
    let origin_events = lifecycle
        .list_spec_sync_queue(feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.uid)
        .collect();
    let previous = lifecycle
        .load_spec_revision(feature_uid)
        .unwrap()
        .expect("fixture has the published product baseline");
    lifecycle
        .append_spec_revision(
            feature_uid,
            previous.seq,
            SpecRevisionDraftV1 {
                content_hash: crate::core::lifecycle::product_spec::current_product_hash(store)
                    .unwrap(),
                source_revision: feature.revision,
                produced_by_run_id: None,
                origin_events,
                decision_refs: feature.decision_refs.clone(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: feature.approved_plan_sha256.clone(),
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                no_change_reason: Some(
                    "Synthetic fixture records the reviewed plan against the unchanged published product specification.".into(),
                ),
                deltas: Vec::new(),
            },
        )
        .unwrap();
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
