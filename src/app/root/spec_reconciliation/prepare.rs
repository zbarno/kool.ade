use super::{evidence_item, spec_binding_hash};
use crate::harness::{ExecutionMode, LiveProgress, PlanningRequest};
use crate::{app::session::Project, core::lifecycle::TaskRecord};
use sha2::Digest as _;
use std::sync::{Arc, atomic::AtomicBool, mpsc};

pub(super) fn prepare(
    project: &Project,
    task: &TaskRecord,
) -> Result<
    (
        PlanningRequest,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
        String,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1};
    let post_completion_pr_refresh =
        task.phase == crate::core::lifecycle::TaskPhase::Done && task.pr_work_item_uid.is_some();
    if (task.phase != crate::core::lifecycle::TaskPhase::AwaitingSpecReconciliation
        && !post_completion_pr_refresh)
        || task.implementation_sha256.is_none()
        || task.reviewed_code_head_sha.is_none()
        || task.reviewed_code_subject_sha256.is_none()
        || task.runtime_qa.as_ref().is_none_or(|receipt| {
            receipt.decision != crate::core::lifecycle::GateDecision::Approved
        })
    {
        return Err("task lacks current passing review, QA, or reviewed-head evidence".into());
    }
    let task_doc = project
        .task_documents
        .iter()
        .find(|doc| {
            doc.identity.as_ref().is_some_and(|identity| {
                Some(identity.uid.as_str()) == task.planning_item_uid.as_deref()
            })
        })
        .ok_or_else(|| "approved task document is unavailable".to_owned())?;
    let expected_task_hash = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&(
                1_u16,
                task.approved_plan_sha256.as_str(),
                task_doc.text.as_str()
            ))
            .map_err(|error| error.to_string())?
        )
    );
    if task.task_spec_sha256.as_deref() != Some(expected_task_hash.as_str()) {
        return Err("current task document differs from the approved lifecycle task".into());
    }
    let feature_text = project
        .state
        .active_features
        .iter()
        .find_map(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .filter(|identity| identity.uid == task.feature_uid)
                .map(|_| markdown.as_str())
        })
        .ok_or_else(|| "approved feature document is unavailable".to_owned())?;
    let feature_id = project
        .state
        .active_features
        .iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == task.feature_uid)
        })
        .map(|(id, _)| id.as_str())
        .ok_or_else(|| "approved feature identity is unavailable".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(feature_text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "approved feature identity is invalid".to_owned())?;
    let contract = crate::core::workflow::feature_contract(feature_text);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        feature_text,
        &contract,
    )
    .map_err(|error| error.to_string())?;
    if snapshot.content_hash != task.approved_plan_sha256
        || !crate::core::lifecycle::plan_snapshot::is_persisted(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
    {
        return Err("current feature differs from the exact approved plan snapshot".into());
    }
    if snapshot.requirements.is_empty() {
        return Err("approved plan has no stable requirements to reconcile".into());
    }
    let current_spec =
        crate::artifacts::product_docs::render_product(&project.state.planning_store)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "current product specification is missing".to_owned())?;
    let spec_sha256 = spec_binding_hash(Some(&current_spec))?;
    let review = task
        .code_review
        .as_ref()
        .ok_or_else(|| "current independent code review receipt is missing".to_owned())?;
    let qa = task
        .qa_execution
        .as_ref()
        .and_then(|state| state.request.as_ref())
        .ok_or_else(|| "current runtime QA request is missing".to_owned())?;
    let qa_store = crate::runtime_qa::RuntimeQaStore::new(
        crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        )
        .map_err(|error| format!("local runtime QA evidence storage is unavailable: {error}"))?,
    );
    let (persisted_request, scenarios, evidence, result) = qa_store
        .load_run(&qa.qa_run_id)
        .map_err(|error| error.to_string())?;
    if &persisted_request != qa {
        return Err("persisted runtime QA report does not match the lifecycle request".into());
    }
    let qa_state = task.qa_execution.as_ref().unwrap();
    let result_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&result).map_err(|error| error.to_string())?,)
    );
    if qa_state.result_sha256.as_deref() != Some(result_hash.as_str())
        || evidence.sha256().map_err(|error| error.to_string())?
            != qa_state
                .evidence_manifest_sha256
                .as_deref()
                .unwrap_or_default()
    {
        return Err(
            "persisted runtime QA evidence differs from the accepted lifecycle report".into(),
        );
    }
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let plan_ref = format!("plan:{}", snapshot.uid);
    let task_ref = format!("task:{}", task.task_uid);
    let review_ref = format!("review:{}", review.receipt_uid);
    let qa_ref = format!("qa:{}", qa.qa_run_id);
    let spec_ref = format!("current-spec:{spec_sha256}");
    let mut evidence_manifest = vec![
        evidence_item(&plan_ref, &workspace_uid, "approved-plan", feature_text)?,
        evidence_item(&task_ref, &workspace_uid, "approved-task", &task_doc.text)?,
        evidence_item(
            &review_ref,
            &workspace_uid,
            "independent-code-review",
            &serde_json::to_string(&(
                &review.receipt_uid,
                &review.subject_sha256,
                &task.reviewed_code_head_sha,
                &task.reviewed_code_subject_sha256,
                &review.evidence_refs,
            ))
            .map_err(|error| error.to_string())?,
        )?,
        evidence_item(
            &qa_ref,
            &workspace_uid,
            "persisted-runtime-qa",
            &serde_json::to_string(&(&persisted_request, &scenarios, &evidence, &result))
                .map_err(|error| error.to_string())?,
        )?,
        evidence_item(
            &spec_ref,
            &workspace_uid,
            "current-product-specification",
            &current_spec,
        )?,
    ];
    if post_completion_pr_refresh {
        let work_uid = task.pr_work_item_uid.as_deref().unwrap();
        let work = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
            .load_pr_preparation(work_uid)
            .map_err(|error| error.to_string())?;
        if work.task_uid != task.task_uid || work.feature_uid != task.feature_uid {
            return Err("provider PR state is bound to a different task or feature".into());
        }
        let pr_ref = format!("provider-pr-state:{}:{}", work.work_item_uid, work.revision);
        let pr_record = serde_json::to_string(&(
            work.status,
            work.provider_pr_id,
            &work.provider_pr_url,
            &work.head_sha,
            &work.base_sha,
            &work.merged_commit_sha,
        ))
        .map_err(|error| error.to_string())?;
        evidence_manifest.push(evidence_item(
            &pr_ref,
            &workspace_uid,
            "provider-reported-pr-state-not-current-checkout",
            &pr_record,
        )?);
    }
    let evidence_ids = evidence_manifest
        .iter()
        .map(|item| item.id.clone())
        .collect();
    let requirement_ids = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role: PromptRole::SpecReconciler,
        workspace_uid,
        feature_uid: task.feature_uid.clone(),
        task_uid: Some(task.task_uid.clone()),
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: vec![review.receipt_uid.clone(), qa.qa_run_id.clone()],
        evidence_manifest,
        execution_policy_ref: "spec-reconciliation/application-gated/v1".into(),
        output_schema_version: PromptRole::SpecReconciler.output_schema_version().into(),
        payload: RolePayloadV1::SpecSync {
            approved_requirements: snapshot
                .requirements
                .iter()
                .map(|requirement| format!("{}: {}", requirement.stable_id, requirement.text))
                .collect(),
            verified_implementation_refs: vec![review_ref, qa_ref, spec_ref],
        },
    })
    .map_err(|error| error.to_string())?;
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: None,
        evidence_ids,
        required_requirements: requirement_ids,
        ..Default::default()
    };
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    Ok((
        PlanningRequest {
            mode: ExecutionMode::Reconciliation,
            task_id: Some(task.task_uid.clone()),
            execution_workspace: None,
            authorization_verifier: None,
            prompt_manifest: Some(rendered.manifest),
            reasoning_level: "high".into(),
            telemetry_phase: Some("spec_reconciliation".into()),
            repo_root: project.state.repo_root.clone(),
            runtime_config_source: None,
            prompt_body: rendered.user,
            system_instructions: rendered.system,
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx,
            cancel: Arc::new(AtomicBool::new(false)),
            review_identity: None,
        },
        context,
        spec_sha256,
    ))
}
