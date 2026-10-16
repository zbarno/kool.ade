use crate::{
    app::session::Project,
    artifacts::{planning_store::PlanningRoot, product_docs},
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, SpecDeltaApprovalV1, TaskPhase,
    },
    harness::prompt_catalog::SpecDeltaV1,
};
use sha2::Digest as _;
use std::collections::BTreeMap;

pub(crate) fn apply_approved_delta(
    project: &mut Project,
    task_uid: &str,
    expected_revision: u64,
    proposal_sha256: &str,
    base_spec_sha256: &str,
) -> Result<(), String> {
    let lifecycle_store = project.state.planning_store.clone();
    let lifecycle = LifecycleStore::new(&lifecycle_store);
    let task = lifecycle
        .load_task(task_uid)
        .map_err(|error| error.to_string())?;
    let current_hash = super::spec_binding_hash(project.state.spec_text.as_deref())?;
    if current_hash != base_spec_sha256 {
        if recover_saved_approval(
            project,
            &lifecycle,
            &task,
            task_uid,
            proposal_sha256,
            base_spec_sha256,
            &current_hash,
        )? {
            return Ok(());
        }
        return Err("The product specification changed since this proposal; rerun reconciliation before approving it.".into());
    }
    if task.revision != expected_revision || task.phase != TaskPhase::AwaitingSpecReconciliation {
        return Err("The lifecycle task changed; reload its current specification proposal before approving it.".into());
    }
    if task.spec_reconciliation_spec_sha256.as_deref() != Some(base_spec_sha256)
        || task.spec_reconciliation_proposal_sha256.as_deref() != Some(proposal_sha256)
    {
        return Err("This specification proposal is stale or no longer current.".into());
    }
    let proposal_json = task
        .spec_reconciliation_proposal_json
        .as_deref()
        .ok_or_else(|| {
            "This blocker has no validated specification proposal to approve.".to_owned()
        })?;
    let actual_hash = format!("{:x}", sha2::Sha256::digest(proposal_json.as_bytes()));
    if actual_hash != proposal_sha256 {
        return Err("The saved specification proposal failed its integrity check.".into());
    }
    let proposal: SpecDeltaV1 =
        serde_json::from_str(proposal_json).map_err(|error| error.to_string())?;
    if proposal.deltas.is_empty() {
        return Err("The saved proposal contains no changes to approve.".into());
    }

    let store = &project.state.planning_store;
    let store_revision = store.revision().map_err(|error| error.to_string())?;
    let persisted_spec = product_docs::render_product(store)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "The modular product specification is unavailable for a safe update.".to_owned()
        })?;
    if super::spec_binding_hash(Some(&persisted_spec))? != base_spec_sha256 {
        return Err("The persisted product specification changed since this proposal; reload and rerun reconciliation.".into());
    }
    let documents = product_docs::load_documents(store)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "The modular product specification is unavailable for a safe update.".to_owned()
        })?;
    let original = documents
        .iter()
        .map(|document| (document.module.id.clone(), document.content.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut updated = documents
        .into_iter()
        .map(|document| (document.module.id.clone(), document))
        .collect::<BTreeMap<_, _>>();

    for delta in &proposal.deltas {
        if !delta.old.contains(&delta.requirement_id) || !delta.new.contains(&delta.requirement_id)
        {
            return Err(format!(
                "The proposed {} change does not preserve its requirement identity.",
                delta.requirement_id
            ));
        }
        let matches = updated
            .iter()
            .filter_map(|(id, document)| {
                let count = document.content.match_indices(&delta.old).count();
                (count > 0).then_some((id.clone(), count))
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0].1 != 1 {
            return Err(format!(
                "The old text for {} does not match exactly one current specification location.",
                delta.requirement_id
            ));
        }
        let document = updated
            .get_mut(&matches[0].0)
            .expect("matched document exists");
        document.content = document.content.replacen(&delta.old, &delta.new, 1);
        product_docs::validate_module(&document.content).map_err(|error| error.to_string())?;
    }

    let layout = store.planning_layout();
    let mut changes = Vec::with_capacity(updated.len() + 1);
    let mut render_updates = Vec::with_capacity(updated.len());
    for document in updated.values() {
        if original.get(&document.module.id) == Some(&document.content) {
            continue;
        }
        let relative = layout
            .product_root()
            .join(&document.module.path)
            .strip_prefix(layout.root())
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        changes.push((relative, document.content.as_bytes().to_vec()));
        render_updates.push((
            format!("product:{}", document.module.id),
            document.content.clone(),
        ));
    }
    if changes.is_empty() {
        return Err("The approved proposal produced no specification changes.".into());
    }
    let updated_spec = product_docs::render_product_with_updates(store, &render_updates)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The updated product specification could not be rendered.".to_owned())?;
    let next_hash = super::spec_binding_hash(Some(&updated_spec))?;
    let receipt_uid = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(task_uid).map_err(|error| error.to_string())?,
        format!("spec-delta-user-approval:v1:{expected_revision}:{proposal_sha256}").as_bytes(),
    )
    .hyphenated()
    .to_string();
    let approval = SpecDeltaApprovalV1 {
        schema_version: 1,
        task_uid: task_uid.to_owned(),
        expected_task_revision: expected_revision,
        proposal_sha256: proposal_sha256.to_owned(),
        base_spec_sha256: base_spec_sha256.to_owned(),
        resulting_spec_sha256: next_hash.clone(),
        receipt: GateReceipt {
            receipt_uid,
            subject_uid: task_uid.to_owned(),
            subject_sha256: proposal_sha256.to_owned(),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: now_ms(),
            evidence_refs: vec![format!(
                "spec-delta:{proposal_sha256}:base:{base_spec_sha256}"
            )],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        },
    };
    approval.validate()?;
    changes.push((
        format!("state/lifecycle_spec_approvals/{task_uid}-{proposal_sha256}.json"),
        serde_json::to_vec_pretty(&approval).map_err(|error| error.to_string())?,
    ));
    store
        .transaction(&changes, Some(&store_revision))
        .map_err(|error| format!("Specification approval could not be committed: {error}"))?;

    project.state.spec_text = Some(updated_spec);
    project.state.baseline_spec = project.state.spec_text.clone();
    if let Err(error) =
        crate::app::root::tick::activity::spec_reconciliation::finish::record_user_approved_delta(
            project, task_uid, &proposal, &approval,
        )
    {
        return Err(format!(
            "Specification and user approval were saved, but the feature revision history could not be updated: {error}"
        ));
    }
    let retry_event = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(task_uid).map_err(|error| error.to_string())?,
        format!("spec-delta-approved:v1:{expected_revision}:{proposal_sha256}:{next_hash}")
            .as_bytes(),
    )
    .hyphenated()
    .to_string();
    if let Err(error) = LifecycleStore::new(&project.state.planning_store)
        .retry_spec_reconciliation(task_uid, expected_revision, &retry_event, next_hash)
    {
        project.activity.pending.push(format!(
            "The approved specification changes were saved, but the reconciliation queue could not be refreshed yet: {error}. It will retry from the saved specification."
        ));
    }
    Ok(())
}

fn recover_saved_approval(
    project: &mut Project,
    lifecycle: &LifecycleStore<'_>,
    task: &crate::core::lifecycle::TaskRecord,
    task_uid: &str,
    proposal_sha256: &str,
    base_spec_sha256: &str,
    current_hash: &str,
) -> Result<bool, String> {
    if task.revision == 0
        || task.phase != TaskPhase::AwaitingSpecReconciliation
        || task.spec_reconciliation_spec_sha256.as_deref() != Some(base_spec_sha256)
        || task.spec_reconciliation_proposal_sha256.as_deref() != Some(proposal_sha256)
    {
        return Ok(false);
    }
    let Ok(approval) = lifecycle.load_spec_delta_approval(task_uid, proposal_sha256) else {
        return Ok(false);
    };
    if approval.base_spec_sha256 != base_spec_sha256
        || approval.resulting_spec_sha256 != current_hash
    {
        return Ok(false);
    }
    let proposal_json = task
        .spec_reconciliation_proposal_json
        .as_deref()
        .ok_or_else(|| "The saved proposal is unavailable for approval recovery.".to_owned())?;
    if format!("{:x}", sha2::Sha256::digest(proposal_json.as_bytes())) != proposal_sha256 {
        return Err(
            "The saved proposal failed its integrity check during approval recovery.".into(),
        );
    }
    let proposal: SpecDeltaV1 =
        serde_json::from_str(proposal_json).map_err(|error| error.to_string())?;
    let persisted_spec = product_docs::render_product(&project.state.planning_store)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The modular product specification is unavailable.".to_owned())?;
    if super::spec_binding_hash(Some(&persisted_spec))? != current_hash {
        return Ok(false);
    }
    project.state.spec_text = Some(persisted_spec);
    project.state.baseline_spec = project.state.spec_text.clone();
    crate::app::root::tick::activity::spec_reconciliation::finish::record_user_approved_delta(
        project, task_uid, &proposal, &approval,
    )
    .map_err(|error| format!("Saved approval history could not be recovered: {error}"))?;
    let retry_event = uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(task_uid).map_err(|error| error.to_string())?,
        format!(
            "spec-delta-approved:v1:{}:{proposal_sha256}:{current_hash}",
            task.revision
        )
        .as_bytes(),
    )
    .hyphenated()
    .to_string();
    lifecycle
        .retry_spec_reconciliation(
            task_uid,
            task.revision,
            &retry_event,
            current_hash.to_owned(),
        )
        .map_err(|error| {
            format!("Saved approval was recovered but retry could not be queued: {error}")
        })?;
    Ok(true)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
