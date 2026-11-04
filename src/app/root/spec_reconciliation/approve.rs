use crate::{
    app::session::Project,
    artifacts::product_docs,
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, SpecDeltaApprovalV1, TaskPhase,
    },
    harness::prompt_catalog::SpecDeltaV1,
};
use sha2::Digest as _;
use std::collections::BTreeMap;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedDocumentationReceipt {
    schema_version: u32,
    pull_request_url: String,
    branch: String,
    base_commit: String,
    head_commit: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingDocumentationPublication {
    schema_version: u32,
    changes: Vec<crate::core::spec_publication::DocumentationChange>,
}

fn publication_paths(task_uid: &str, proposal_sha256: &str) -> (String, String) {
    (
        format!("state/spec_publications/{task_uid}-{proposal_sha256}.json"),
        format!("state/spec_publications/pending/{task_uid}-{proposal_sha256}.json"),
    )
}

pub(crate) fn publish_approved_documentation(
    project: &mut Project,
    task_uid: &str,
    proposal_sha256: &str,
) -> Result<String, String> {
    if uuid::Uuid::parse_str(task_uid).is_err()
        || proposal_sha256.len() != 64
        || !proposal_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("The approved documentation identity is invalid.".into());
    }
    let store = &project.state.planning_store;
    let (receipt_path, pending_path) = publication_paths(task_uid, proposal_sha256);
    if let Ok(bytes) = store.read(&receipt_path) {
        let receipt: PublishedDocumentationReceipt = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Saved publication receipt is invalid: {error}"))?;
        if receipt.schema_version != 1 {
            return Err("Saved publication receipt has an unsupported schema.".into());
        }
        let _ = store.remove(&pending_path);
        record_publication_notice(project, &receipt.pull_request_url);
        return Ok(receipt.pull_request_url);
    }
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| "There is no pinned published specification to update.".to_owned())?;
    let bytes = store
        .read(&pending_path)
        .map_err(|error| format!("Approved documentation publication is unavailable: {error}"))?;
    let pending: PendingDocumentationPublication = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Approved documentation publication is invalid: {error}"))?;
    if pending.schema_version != 1 || pending.changes.is_empty() {
        return Err(
            "Approved documentation publication has an unsupported schema or no changes.".into(),
        );
    }
    let changes = pending.changes;
    let consent = crate::core::spec_publication::PublicationConsent::confirmed(&changes)
        .map_err(|error| error.to_string())?;
    let pull_request = crate::core::spec_publication::publish_approved(
        &project.state.repo_root,
        &pin.repository_id,
        pin,
        &changes,
        &consent,
    )
    .map_err(|error| {
        format!(
            "The approval remains private, but its documentation PR could not be opened: {error}"
        )
    })?;
    let receipt = PublishedDocumentationReceipt {
        schema_version: 1,
        pull_request_url: pull_request.url.clone(),
        branch: pull_request.branch,
        base_commit: pull_request.base_commit,
        head_commit: pull_request.head_commit,
    };
    store
        .atomic_write(
            &receipt_path,
            &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?,
        )
        .map_err(|error| {
            format!(
                "Documentation PR {} was opened, but its private receipt could not be saved: {error}",
                receipt.pull_request_url
            )
        })?;
    store
        .remove(&pending_path)
        .map_err(|error| format!("Documentation PR opened, but its private pending publication could not be removed: {error}"))?;
    record_publication_notice(project, &receipt.pull_request_url);
    Ok(receipt.pull_request_url)
}

fn record_publication_notice(project: &mut Project, url: &str) {
    let notice = format!(
        "Approved documentation is proposed in {url}; merge the protected PR before treating it as published."
    );
    if !project
        .activity
        .pending
        .iter()
        .any(|event| event == &notice)
    {
        project.activity.pending.push(notice);
    }
}

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
    let pin = project
        .state
        .published_spec_pin
        .as_ref()
        .ok_or_else(|| "There is no pinned published specification to update.".to_owned())?;
    let reader = crate::core::published_specs::PublishedSpecReader::new(
        project.state.repo_root.clone(),
        pin.repository_id.clone(),
        pin.reference.clone(),
    )
    .map_err(|error| error.to_string())?;
    reader
        .ensure_local_ref_unchanged(pin)
        .map_err(|error| error.to_string())?;
    let persisted_spec = product_docs::render_published_product(&reader, pin)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "The pinned code repository has no published product documents to update.".to_owned()
        })?;
    if super::spec_binding_hash(Some(&persisted_spec))? != base_spec_sha256 {
        return Err("The persisted product specification changed since this proposal; reload and rerun reconciliation.".into());
    }
    let (_, documents) =
        product_docs::load_published_product(&reader, pin).map_err(|error| error.to_string())?;
    if documents.is_empty() {
        return Err(
            "The pinned code repository has no published product documents to update.".into(),
        );
    }
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

    let mut changes = Vec::with_capacity(2);
    let mut render_updates = Vec::with_capacity(updated.len());
    let mut published_changes = Vec::with_capacity(updated.len());
    for document in updated.values() {
        if original.get(&document.module.id) == Some(&document.content) {
            continue;
        }
        published_changes.push(crate::core::spec_publication::DocumentationChange {
            path: format!("docs/koolade/product/{}", document.module.path),
            content: document.content.clone(),
        });
        render_updates.push((
            format!("product:{}", document.module.id),
            document.content.clone(),
        ));
    }
    published_changes.push(crate::core::spec_publication::DocumentationChange {
        path: "docs/koolade/product/index.md".into(),
        content: format!(
            "# Product\n\n{}\n",
            updated
                .values()
                .map(|document| {
                    format!("- [{}](./{})", document.module.title, document.module.path)
                })
                .collect::<Vec<_>>()
                .join("\n")
        ),
    });
    if render_updates.is_empty() {
        return Err("The approved proposal produced no specification changes.".into());
    }
    let updated_spec =
        product_docs::render_published_product_with_updates(&reader, pin, &render_updates)
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
    let (_, pending_path) = publication_paths(task_uid, proposal_sha256);
    changes.push((
        pending_path,
        serde_json::to_vec_pretty(&PendingDocumentationPublication {
            schema_version: 1,
            changes: published_changes,
        })
        .map_err(|error| error.to_string())?,
    ));
    store
        .transaction(&changes, Some(&store_revision))
        .map_err(|error| format!("Specification approval could not be committed: {error}"))?;

    project.state.spec_text = Some(updated_spec);
    project.state.baseline_spec = project.state.spec_text.clone();
    publish_approved_documentation(project, task_uid, proposal_sha256)?;
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
    publish_approved_documentation(project, task_uid, proposal_sha256)?;
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
