use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use sha2::{Digest, Sha256};

use crate::{
    app::session::Project,
    artifacts::{
        planning_store::PlanningStore,
        task_docs::{TaskDocument, TaskMetadata},
    },
    core::{
        implementation::Implementation,
        lifecycle::{GateActor, GateDecision, GateReceipt, TaskPhase, TaskRecord},
    },
    domain::ArtifactIdentity,
    persistence::runtime_target_approvals::WorkspaceCleanup,
    project_runtime::{
        AppSurface, AppTargetV1, ProjectCapabilityManifestV1, ProjectRuntimeStore,
        ReadinessProbeV1, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
        TargetCapabilityState,
    },
};

#[path = "desktop_artifact/fixtures.rs"]
mod fixtures;
use fixtures::{approved_recipe, sha256, source_sha};

pub(crate) struct DesktopArtifactFixture {
    pub project: Project,
    pub task: TaskRecord,
    _project_root: tempfile::TempDir,
    _task_clone: tempfile::TempDir,
    _approval_cleanup: WorkspaceCleanup,
}

impl DesktopArtifactFixture {
    pub(crate) fn from_executable(executable: &Path) -> Self {
        let project_root = tempfile::tempdir().unwrap();
        let task_clone = tempfile::tempdir().unwrap();
        let artifact_relative = PathBuf::from("dist/approved-desktop-app");
        let artifact_path = task_clone.path().join(&artifact_relative);
        std::fs::create_dir_all(artifact_path.parent().unwrap()).unwrap();
        std::fs::copy(executable, &artifact_path).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&artifact_path, std::fs::Permissions::from_mode(0o555)).unwrap();
        let artifact_sha = format!(
            "{:x}",
            Sha256::digest(std::fs::read(&artifact_path).unwrap())
        );

        let project_id = uuid::Uuid::new_v4();
        let workspace_uid = project_id.hyphenated().to_string();
        let planning = PlanningStore::legacy_embedded(project_id, project_root.path());
        let mut project = crate::app::session::test_support::project_from(project_root.path());
        project.state.planning_store = planning.clone();
        let approval_cleanup = WorkspaceCleanup::new(&workspace_uid);

        let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let planning_item_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let repository_id = "qa-fixture-repo";
        let source_sha = source_sha();
        let task_text = "# Approved desktop app\n\n## Acceptance criteria\n\n- The application opens and responds to pointer and keyboard input.\n";
        let identity = ArtifactIdentity {
            uid: planning_item_uid.clone(),
            display_id: "TASK-QA-001".into(),
            title: "Approved desktop app".into(),
            parent_uid: Some(batch_uid),
        };
        let metadata = TaskMetadata::new(&identity, repository_id, Vec::new()).unwrap();
        let document_path = "docs/tasks/approved-desktop-app.md".to_owned();
        project.task_documents.push(TaskDocument {
            path: document_path.clone(),
            title: "Approved desktop app".into(),
            text: task_text.into(),
            identity: Some(identity),
            metadata: Some(metadata),
            task_state: None,
            metadata_error: None,
        });

        let implementation = serde_json::from_value::<Implementation>(serde_json::json!({
            "ticket": document_path,
            "task_uid": planning_item_uid,
            "ticket_text": task_text,
            "workspace_uid": workspace_uid,
            "branch": "feat/approved-desktop-fixture",
            "base": "main",
            "base_commit": source_sha,
            "task_repository": task_clone.path(),
            "status": "completed",
            "detail": "approved desktop artifact fixture",
            "pr_url": null,
            "verified_head": source_sha,
            "repository_id": repository_id
        }))
        .unwrap();
        project
            .implementation_states
            .insert(document_path.clone(), implementation);

        let executable_id = "approved-desktop-app";
        let build_recipe = approved_recipe(
            "qa-desktop-fixture-build",
            "cargo",
            vec!["build".into()],
            vec![artifact_relative.to_string_lossy().into_owned()],
        );
        let start_recipe = approved_recipe(
            "qa-desktop-fixture-start",
            executable_id,
            vec!["--qa-fixture".into()],
            Vec::new(),
        );
        let target = AppTargetV1 {
            target_id: "qa-desktop-fixture-target".into(),
            owning_task_id: planning_item_uid.clone(),
            repository_id: repository_id.into(),
            app_surface: AppSurface::LinuxDesktop,
            project_relative_root: ".".into(),
            build_recipe_ref: Some(build_recipe.recipe_id.clone()),
            test_recipe_refs: Vec::new(),
            start_recipe_ref: Some(start_recipe.recipe_id.clone()),
            readiness_probe_ref: Some("qa-desktop-fixture-ready".into()),
            artifact_spec_ref: Some("qa-desktop-fixture-artifacts".into()),
            fixture_ref: None,
            required_dependencies: Vec::new(),
            supported_driver_ids: Vec::new(),
            port_needs: Vec::new(),
            environment_policy_ref: "default-deny".into(),
            capability_state: TargetCapabilityState::ReadyButUntested,
        };
        let manifest = ProjectCapabilityManifestV1 {
            schema_version: 1,
            workspace_id: workspace_uid.clone(),
            repository_id: repository_id.into(),
            source_sha: source_sha.clone(),
            detected_languages: Vec::new(),
            frameworks: Vec::new(),
            candidate_targets: vec![target.clone()],
            selected_target_id: Some(target.target_id.clone()),
            target_capabilities: vec![TargetCapabilityState::ReadyButUntested],
            recipes: Vec::new(),
            readiness_probes: [(
                "qa-desktop-fixture-ready".into(),
                ReadinessProbeV1::ProcessAlive {
                    executable_id: executable_id.into(),
                },
            )]
            .into_iter()
            .collect(),
            artifact_specs: [(
                "qa-desktop-fixture-artifacts".into(),
                vec![artifact_relative.to_string_lossy().into_owned()],
            )]
            .into_iter()
            .collect(),
            confidence: 100,
            evidence_refs: Vec::new(),
            user_overrides: vec![build_recipe.clone(), start_recipe.clone()],
            revision: 0,
        };
        let runtime = ProjectRuntimeStore::new(&planning);
        runtime.save_manifest(&manifest, 0).unwrap();
        let saved_manifest = runtime.load_manifest().unwrap().unwrap();
        for (operation, recipe) in [
            (RuntimeOperation::Build, &build_recipe),
            (RuntimeOperation::Start, &start_recipe),
        ] {
            let binding = ProjectRuntimeStore::user_recipe_approval_binding(
                &saved_manifest,
                &target,
                operation,
                recipe,
            )
            .unwrap();
            crate::persistence::runtime_target_approvals::save(&binding).unwrap();
        }

        let artifact_hashes = BTreeMap::from([(
            artifact_relative.to_string_lossy().into_owned(),
            artifact_sha,
        )]);
        for (operation, recipe, observed_at_unix_ms) in [
            (RuntimeOperation::Build, &build_recipe, 1),
            (RuntimeOperation::Start, &start_recipe, 2),
        ] {
            runtime
                .append_receipt(&RuntimeReceiptV1 {
                    schema_version: 1,
                    receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                    operation,
                    task_id: planning_item_uid.clone(),
                    target_id: target.target_id.clone(),
                    source_sha: source_sha.clone(),
                    recipe_id: Some(recipe.recipe_id.clone()),
                    state: RuntimeReceiptState::Passed,
                    exit_code: Some(0),
                    artifact_hashes: artifact_hashes.clone(),
                    prepared_dependency_ids: Vec::new(),
                    log_reference: None,
                    observed_at_unix_ms,
                })
                .unwrap();
        }

        let approved_plan_sha = sha256(b"approved desktop fixture plan");
        let task_spec_sha = {
            let bytes =
                serde_json::to_vec(&(1_u16, approved_plan_sha.as_str(), task_text)).unwrap();
            format!("{:x}", Sha256::digest(bytes))
        };
        let diff_sha = sha256(b"reviewed desktop fixture diff");
        let review_subject_sha = sha256(b"reviewed desktop fixture subject");
        let review = GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: task_uid.clone(),
            subject_sha256: diff_sha.clone(),
            actor: GateActor::IndependentReviewer,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 3,
            evidence_refs: vec!["fixture/reviewed-desktop-app".into()],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            author_run_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            author_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        };
        let task = TaskRecord {
            schema_version: 1,
            task_uid,
            feature_uid,
            planning_item_uid: Some(planning_item_uid),
            task_spec_sha256: Some(task_spec_sha),
            parent_task_uid: None,
            subtask_uids: Vec::new(),
            dependency_task_uids: Vec::new(),
            approved_requirement_ids: Vec::new(),
            phase: TaskPhase::AwaitingRuntimeQa,
            revision: 4,
            approved_plan_sha256: approved_plan_sha,
            implementation_sha256: Some(diff_sha),
            reviewed_code_head_sha: Some(source_sha),
            reviewed_code_subject_sha256: Some(review_subject_sha),
            code_review: Some(review),
            review_history: Vec::new(),
            repair_attempts: 0,
            runtime_qa: None,
            qa_execution: None,
            spec_reconciliation: None,
            spec_reconciliation_blocker: None,
            spec_reconciliation_proposal_sha256: None,
            spec_reconciliation_spec_sha256: None,
            spec_reconciliation_proposal_json: None,
            pr_work_item_uid: None,
        };

        Self {
            project,
            task,
            _project_root: project_root,
            _task_clone: task_clone,
            _approval_cleanup: approval_cleanup,
        }
    }
}
