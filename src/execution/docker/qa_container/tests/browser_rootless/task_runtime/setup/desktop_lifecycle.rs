use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{
    app::session::Project,
    artifacts::task_docs::{TaskDocument, TaskMetadata},
    core::{
        implementation::Implementation,
        lifecycle::{GateActor, GateDecision, GateReceipt, TaskPhase, TaskRecord},
    },
    domain::ArtifactIdentity,
    execution::contract::WorkspaceRef,
    project_runtime::AppSurface,
};

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn reviewed_task(
    project: &mut Project,
    task_clone: &Path,
    workspace: &WorkspaceRef,
    diff_sha: &str,
    base_sha: &str,
    repository_id: &str,
    app_surface: AppSurface,
) -> TaskRecord {
    let planning_item_uid = workspace.task_id.clone();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let (document_path, task_text, detail) = match app_surface {
        AppSurface::Web => (
            "docs/tasks/web-runtime-qa.md",
            "# Web runtime task\n\n## Acceptance criteria\n\n- Submitting a non-empty message displays Saved.\n",
            "source-built web runtime QA fixture",
        ),
        AppSurface::HeadlessService => (
            "docs/tasks/headless-runtime-qa.md",
            "# Headless service task\n\n## Acceptance criteria\n\n- POST /save returns saved=true.\n",
            "source-built headless service runtime QA fixture",
        ),
        AppSurface::LinuxDesktop => (
            "docs/tasks/rust-desktop-runtime-qa.md",
            "# Rust desktop task\n\n## Acceptance criteria\n\n- The native application opens and responds to pointer and keyboard input.\n",
            "source-built Rust desktop runtime QA fixture",
        ),
        _ => unreachable!("runtime lifecycle fixtures use supported surfaces"),
    };
    let document_path = document_path.to_owned();
    let identity = ArtifactIdentity {
        uid: planning_item_uid.clone(),
        display_id: "TASK-QA-DESKTOP-001".into(),
        title: match app_surface {
            AppSurface::Web => "Web runtime QA fixture",
            AppSurface::HeadlessService => "Headless runtime QA fixture",
            AppSurface::LinuxDesktop => "Rust desktop runtime QA fixture",
            _ => unreachable!("runtime lifecycle fixtures use supported surfaces"),
        }
        .into(),
        parent_uid: Some(batch_uid),
    };
    let metadata = TaskMetadata::new(&identity, repository_id, Vec::new()).unwrap();
    project.task_documents.push(TaskDocument {
        path: document_path.clone(),
        title: identity.title.clone(),
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
        "workspace_uid": workspace.project_id,
        "branch": "feat/runtime-qa-fixture",
        "base": "main",
        "base_commit": base_sha,
        "task_repository": task_clone,
        "status": "completed",
        "detail": detail,
        "pr_url": null,
        "verified_head": workspace.source_sha,
        "repository_id": repository_id
    }))
    .unwrap();
    project
        .implementation_states
        .insert(document_path, implementation);

    let approved_plan_sha = sha256(match app_surface {
        AppSurface::Web => b"approved web task fixture plan",
        AppSurface::HeadlessService => b"approved headless task fixture plan",
        AppSurface::LinuxDesktop => b"approved Rust desktop task fixture plan",
        _ => unreachable!("runtime lifecycle fixtures use supported surfaces"),
    });
    let task_spec_sha = {
        let bytes = serde_json::to_vec(&(1_u16, approved_plan_sha.as_str(), task_text)).unwrap();
        sha256(&bytes)
    };
    let review_subject_sha = {
        let bytes = serde_json::to_vec(&(
            workspace.source_sha.as_str(),
            diff_sha,
            approved_plan_sha.as_str(),
        ))
        .unwrap();
        sha256(&bytes)
    };
    let review = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.clone(),
        subject_sha256: diff_sha.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 3,
        evidence_refs: vec!["fixture/reviewed-runtime-qa-task".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        author_run_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        author_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    TaskRecord {
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
        implementation_sha256: Some(diff_sha.into()),
        reviewed_code_head_sha: Some(workspace.source_sha.clone()),
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
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
