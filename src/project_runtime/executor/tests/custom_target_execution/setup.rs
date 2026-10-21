use std::{path::PathBuf, sync::Arc};

use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    execution::{
        contract::{ExecutionModeV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef},
        session::ExecutionSession,
    },
    persistence::runtime_target_approvals::Binding,
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement,
        ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, TargetCapabilityState,
    },
    ui::layout::RuntimeTargetApprovalProof,
};

use super::backend::CapturingDockerBackend;

pub(super) struct Fixture {
    root: PathBuf,
    pub planning: PlanningStore,
    pub backend: Arc<CapturingDockerBackend>,
    pub session: ExecutionSession,
    pub workspace_id: String,
    pub task_id: String,
    pub source_sha: String,
    pub target_id: String,
    _approval_cleanup: crate::persistence::runtime_target_approvals::WorkspaceCleanup,
    pub local_binding: Binding,
    pub receipt_id: String,
    pub recipe_approval_id: String,
    pub digest: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn build() -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "koolade-custom-target-exec-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let planning = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
    let store = ProjectRuntimeStore::new(&planning);
    let workspace_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let approval_cleanup =
        crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&workspace_id);
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = "a".repeat(40);
    let mut manifest = ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: workspace_id.clone(),
        repository_id: "synthetic-repo".into(),
        source_sha: source_sha.clone(),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: Vec::new(),
        selected_target_id: None,
        target_capabilities: Vec::new(),
        recipes: Vec::new(),
        readiness_probes: Default::default(),
        artifact_specs: Default::default(),
        confidence: 0,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 0,
    };
    store.save_manifest(&manifest, 0).unwrap();
    manifest.revision = 1;

    let approval_proof = RuntimeTargetApprovalProof::for_test();
    let receipt_id = approval_proof.receipt_uid();
    let recipe_approval_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let target = AppTargetV1 {
        target_id: format!("user-target-{}", uuid::Uuid::new_v4().simple()),
        owning_task_id: task_id.clone(),
        repository_id: manifest.repository_id.clone(),
        app_surface: AppSurface::Web,
        project_relative_root: ".".into(),
        build_recipe_ref: Some("synthetic-build".into()),
        test_recipe_refs: Vec::new(),
        start_recipe_ref: None,
        readiness_probe_ref: None,
        artifact_spec_ref: Some("custom-artifacts".into()),
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "synthetic-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["build.js".into(), "--release".into()],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: vec!["dist/index.html".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![
            format!("user-approval:{receipt_id}"),
            format!("user-recipe-approval:{recipe_approval_id}"),
        ],
    };
    let proposal_sha256 = ProjectRuntimeStore::user_target_proposal_sha256(
        &manifest.workspace_id,
        &manifest.repository_id,
        &manifest.source_sha,
        &target,
        std::slice::from_ref(&recipe),
        None,
        Some(&vec!["dist/index.html".into()]),
    )
    .unwrap();
    let approval = receipt(
        receipt_id.clone(),
        &task_id,
        proposal_sha256.clone(),
        &target.target_id,
    );
    let origin_sha256 =
        ProjectRuntimeStore::custom_target_origin_sha256(&manifest, &target).unwrap();

    // A stale manifest transaction cannot leave behind local origin state.
    let stale_proof = RuntimeTargetApprovalProof::for_test();
    let stale_receipt = receipt(
        stale_proof.receipt_uid(),
        &task_id,
        proposal_sha256.clone(),
        &target.target_id,
    );
    assert!(
        store
            .add_user_target_with_readiness_and_save(
                target.clone(),
                vec![recipe.clone()],
                None,
                vec!["dist/index.html".into()],
                stale_receipt,
                stale_proof,
                0,
            )
            .is_err()
    );
    assert!(
        !crate::persistence::runtime_target_approvals::has_custom_target_origin(
            &manifest.workspace_id,
            &task_id,
            &origin_sha256,
        )
        .unwrap()
    );

    let target_id = target.target_id.clone();
    let local_binding = Binding {
        workspace_id: manifest.workspace_id.clone(),
        task_id: task_id.clone(),
        repository_id: manifest.repository_id.clone(),
        source_sha: manifest.source_sha.clone(),
        target_id: target_id.clone(),
        proposal_sha256,
        receipt_id: receipt_id.clone(),
    };
    let saved = store
        .add_user_target_with_readiness_and_save(
            target,
            vec![recipe],
            None,
            vec!["dist/index.html".into()],
            approval,
            approval_proof,
            1,
        )
        .unwrap();
    let mut selected = saved;
    selected.selected_target_id = Some(target_id.clone());
    store.save_manifest(&selected, 2).unwrap();

    let backend = Arc::new(CapturingDockerBackend::default());
    let digest = format!("sha256:{}", "b".repeat(64));
    let session = session(
        backend.clone(),
        workspace_id.clone(),
        task_id.clone(),
        source_sha.clone(),
        digest.clone(),
    );
    Fixture {
        root,
        planning,
        backend,
        session,
        workspace_id,
        task_id,
        source_sha,
        target_id,
        _approval_cleanup: approval_cleanup,
        local_binding,
        receipt_id,
        recipe_approval_id,
        digest,
    }
}

pub(super) fn receipt(
    receipt_id: String,
    task_id: &str,
    proposal_sha256: String,
    target_id: &str,
) -> GateReceipt {
    GateReceipt {
        receipt_uid: receipt_id,
        subject_uid: task_id.to_owned(),
        subject_sha256: proposal_sha256,
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: super::backend::now_ms(),
        evidence_refs: vec![format!("runtime-target:{target_id}")],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

pub(super) fn session(
    backend: Arc<CapturingDockerBackend>,
    workspace_id: String,
    task_id: String,
    source_sha: String,
    digest: String,
) -> ExecutionSession {
    ExecutionSession::new(
        backend,
        WorkspaceRef {
            project_id: workspace_id,
            task_id,
            source_sha,
        },
        ExecutionTarget::local(),
        ExecutionModeV1::Verification,
        digest,
        ResourceBudgetV1 {
            cpu_millis: 1000,
            memory_bytes: 1024 * 1024 * 1024,
            process_count: 32,
            temporary_disk_bytes: 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024,
        },
    )
    .unwrap()
}
