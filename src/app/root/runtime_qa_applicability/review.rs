use super::super::{KooladeApp, Screen};

pub(super) fn start(app: &mut KooladeApp, ticket: &str) {
    let prepared = match &app.screen {
        Screen::Connected(project) => prepare(project, ticket),
        Screen::Welcome => Err("Connect a project before reviewing QA applicability.".into()),
    };
    let (assessment, revision, workspace_uid, feature_uid, context, repo_root) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            app.toasts.warning(error);
            return;
        }
    };
    let Screen::Connected(project) = &mut app.screen else {
        return;
    };
    if project.activity.runtime_qa_applicability_review.is_some() {
        app.toasts
            .warning("A runtime QA applicability review is already running.");
        return;
    }
    let harness = crate::app::root::configured_harness_for_project_task(
        &mut app.task_harness,
        Some(crate::persistence::harness_settings::QA),
        &project.state.harness_routes.routes,
        &Default::default(),
    );
    project.activity.runtime_qa_applicability_review = Some(
        crate::runtime_qa::applicability_review::ApplicabilityReviewHandle::start(
            assessment,
            revision,
            workspace_uid,
            feature_uid,
            context,
            repo_root,
            harness,
        ),
    );
    app.toasts.success(
        "Started an independent review of the headless visual QA applicability assessment.",
    );
}

fn prepare(
    project: &crate::app::session::Project,
    ticket: &str,
) -> Result<
    (
        crate::runtime_qa::QaApplicabilityAssessmentV1,
        u64,
        String,
        String,
        String,
        std::path::PathBuf,
    ),
    String,
> {
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .ok_or_else(|| "Task details no longer identify a saved task.".to_owned())?;
    let metadata = document
        .metadata
        .as_ref()
        .ok_or_else(|| "Task has no persisted runtime identity.".to_owned())?;
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .task_for_planning_item(&metadata.uid)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task lifecycle record is unavailable.".to_owned())?;
    if task.phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa {
        return Err(
            "Headless applicability review is only available while a task awaits runtime QA."
                .into(),
        );
    }
    let implementation = project
        .implementation_states
        .get(ticket)
        .ok_or_else(|| "The verified task implementation is unavailable.".to_owned())?;
    let code_sha = task
        .reviewed_code_head_sha
        .as_deref()
        .filter(|sha| Some(*sha) == implementation.verified_head.as_deref())
        .ok_or_else(|| "The task has no current independently reviewed code head.".to_owned())?;
    let manifest = crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Discover project targets before assessing QA applicability.".to_owned())?;
    if manifest.workspace_id != implementation.workspace_uid.as_deref().unwrap_or_default()
        || manifest.repository_id != metadata.repository_id
        || manifest.source_sha != code_sha
    {
        return Err("The target manifest belongs to a different workspace, repository, or reviewed source revision.".into());
    }
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| {
            target.app_surface == crate::project_runtime::AppSurface::HeadlessService
                && manifest.is_active_target(&metadata.uid, &target.target_id)
                && target.repository_id == metadata.repository_id
        })
        .ok_or_else(|| {
            "Select an active headless service target before reviewing applicability.".to_owned()
        })?;
    let runtime_store =
        crate::project_runtime::ProjectRuntimeStore::new(&project.state.planning_store);
    let service_receipts = runtime_store
        .headless_service_test_receipts(&metadata.uid, target, code_sha)
        .map_err(|error| error.to_string())?;
    let approved_spec_hash = task.approved_plan_sha256.clone();
    let manifest_hash = sha256(&serde_json::to_vec(&manifest).map_err(|error| error.to_string())?);
    let mut evidence_refs = vec![
        format!("target:{}", target.target_id),
        format!("runtime-manifest:{manifest_hash}"),
        format!("reviewed-code:{code_sha}"),
        format!("approved-spec:{approved_spec_hash}"),
    ];
    if let Some(receipt) = task.code_review.as_ref() {
        evidence_refs.push(format!("gate-receipt:{}", receipt.receipt_uid));
    }
    evidence_refs.extend(
        service_receipts
            .iter()
            .map(|receipt| format!("runtime-receipt:{}", receipt.receipt_id)),
    );
    let review_context = format!(
        "{}\n\nExact persisted headless build and test receipts:\n{}",
        document.text,
        serde_json::to_string(&service_receipts).map_err(|error| error.to_string())?
    );
    let assessment = crate::runtime_qa::QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: task.task_uid.clone(),
        target_id: target.target_id.clone(),
        app_surface: target.app_surface,
        code_sha: code_sha.to_owned(),
        approved_spec_hash,
        applicability: crate::runtime_qa::QaApplicability::NotApplicable,
        reason: "The selected target is a headless service with no visual interface. This decision does not certify API, service, or integration test coverage.".into(),
        evidence_refs,
        reviewer_execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        reviewer_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        approved_by: uuid::Uuid::new_v4().hyphenated().to_string(),
        manager_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        recorded_at_unix_ms: now_ms(),
    };
    assessment.validate()?;
    Ok((
        assessment,
        task.revision,
        manifest.workspace_id,
        task.feature_uid,
        review_context,
        project.state.repo_root.clone(),
    ))
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(1)
}
