use super::Snapshot;
use crate::{
    artifacts::{items_io, planning_store::PlanningStore},
    core::{
        project_repos::{ProjectManifest, Repository, portable_remote},
        state::PlannerState,
    },
    domain::OpenItem,
};

pub(super) fn write_normalized_records(
    store: &PlanningStore,
    snapshot: &Snapshot,
) -> anyhow::Result<()> {
    let mut work = snapshot.work.clone();
    for record in &mut work {
        record.record_revision = 0;
        record.record_baseline = None;
    }
    let work_files = store.list_files(crate::artifacts::planning_store::paths::WORK_RECORDS)?;
    if !work_files.is_empty() {
        anyhow::ensure!(
            crate::core::planning_work::load(store)? == work,
            "staged work records conflict with the source inventory"
        );
    } else {
        let (changes, checks, _) = crate::core::planning_work::record_changes(store, &work)?;
        apply_records(store, changes, checks)?;
    }

    let mut workflow = snapshot.state.workflow.clone();
    workflow.record_revisions.clear();
    workflow.record_baselines.clear();
    normalize_workflow_paths(&mut workflow);
    let workflow_files =
        store.list_files(crate::artifacts::planning_store::paths::WORKFLOW_RECORDS)?;
    if !workflow_files.is_empty() {
        anyhow::ensure!(
            workflow_matches(
                &crate::artifacts::task_docs::load_workflow(store)?,
                &workflow
            ),
            "staged workflow records conflict with the source inventory"
        );
    } else {
        let (changes, checks, _) =
            crate::artifacts::task_docs::workflow_record_changes(store, &workflow)?;
        apply_records(store, changes, checks)?;
    }

    let mut open = snapshot.open_items.clone();
    let mut resolved = snapshot.resolved_items.clone();
    for item in open.iter_mut().chain(resolved.iter_mut()) {
        item.record_revision = 0;
        item.record_baseline = None;
    }
    let item_files = store.list_files(crate::artifacts::planning_store::paths::ITEM_STATES)?;
    if !item_files.is_empty() {
        let (current_open, current_resolved, _) = items_io::load_store(store)?;
        let (expected_open, expected_resolved) = normalize_item_uids(open, resolved)?;
        anyhow::ensure!(
            current_open == expected_open && current_resolved == expected_resolved,
            "staged item records conflict with the source inventory"
        );
    } else {
        let (changes, checks, _) = items_io::record_changes(store, &open, &resolved)?;
        apply_records(store, changes, checks)?;
    }
    Ok(())
}

fn apply_records(
    store: &PlanningStore,
    changes: Vec<(String, Vec<u8>)>,
    checks: Vec<crate::artifacts::planning_store::RecordRevisionCheck>,
) -> anyhow::Result<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let revision = store.revision()?;
    store.transaction_with_revision_and_record_revisions(&changes, Some(&revision), &checks)?;
    Ok(())
}

fn normalize_item_uids(
    mut open: Vec<OpenItem>,
    mut resolved: Vec<OpenItem>,
) -> anyhow::Result<(Vec<OpenItem>, Vec<OpenItem>)> {
    use sha2::{Digest, Sha256};
    for item in open.iter_mut().chain(resolved.iter_mut()) {
        let uid = match &item.uid {
            Some(uid) => uuid::Uuid::parse_str(uid)?.hyphenated().to_string(),
            None => {
                let mut bytes: [u8; 16] = Sha256::digest(item.id.as_bytes())[..16]
                    .try_into()
                    .expect("SHA-256 provides 16 bytes");
                bytes[6] = (bytes[6] & 0x0f) | 0x50;
                bytes[8] = (bytes[8] & 0x3f) | 0x80;
                uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
            }
        };
        item.uid = Some(uid);
        item.record_revision = 0;
        item.record_baseline = None;
    }
    Ok((open, resolved))
}

fn normalize_workflow_paths(workflow: &mut crate::core::workflow::Workflow) {
    for batch in &mut workflow.task_batches {
        let directory = batch
            .directory
            .strip_prefix(".koolade-packet/")
            .unwrap_or(&batch.directory);
        batch.directory = if let Some(suffix) = directory.strip_prefix("planning/features/") {
            format!("planning/changes/{suffix}")
        } else {
            directory.to_owned()
        };
    }
}

pub(super) fn workflow_matches(
    left: &crate::core::workflow::Workflow,
    right: &crate::core::workflow::Workflow,
) -> bool {
    fn comparable(
        mut workflow: crate::core::workflow::Workflow,
    ) -> crate::core::workflow::Workflow {
        workflow.record_revisions.clear();
        workflow.record_baselines.clear();
        for batch in &mut workflow.task_batches {
            batch.created_at_ms = 0;
        }
        workflow
    }
    comparable(left.clone()) == comparable(right.clone())
}

pub(super) fn expected_workflow(snapshot: &Snapshot) -> crate::core::workflow::Workflow {
    let mut workflow = snapshot.state.workflow.clone();
    workflow.record_revisions.clear();
    workflow.record_baselines.clear();
    normalize_workflow_paths(&mut workflow);
    workflow
}

pub(super) fn ensure_root_repository(
    manifest: &mut ProjectManifest,
    code_root: &std::path::Path,
) -> anyhow::Result<()> {
    let actual_remote = super::super::super::super::git::origin(code_root).unwrap_or_default();
    if let Some(root) = manifest.repositories.iter().find(|repo| repo.id == "root") {
        anyhow::ensure!(
            root.remote.is_empty() || actual_remote.is_empty() || root.remote == actual_remote,
            "legacy root repository remote does not match the connected code checkout"
        );
    } else {
        manifest.repositories.push(Repository {
            id: "root".into(),
            role: "Planning root".into(),
            remote: if portable_remote(&actual_remote) {
                actual_remote
            } else {
                String::new()
            },
            display_name: None,
        });
    }
    manifest.normalize_display_names()?;
    manifest.validate()
}

pub(super) fn audit_tasks(store: &PlanningStore, state: &PlannerState) -> anyhow::Result<()> {
    let documents = crate::artifacts::task_docs::load_board(store, &state.workflow);
    let mut uids = std::collections::BTreeSet::new();
    for document in documents {
        if let Some(error) = document.metadata_error {
            anyhow::bail!("task-status audit failed at {}: {error}", document.path);
        }
        if let Some(identity) = document.identity {
            anyhow::ensure!(
                uids.insert(identity.uid.clone()),
                "duplicate task UID {}",
                identity.uid
            );
        }
    }
    Ok(())
}

pub(super) fn verify_typed_state(
    snapshot: &Snapshot,
    target: &PlannerState,
    store: &PlanningStore,
) -> anyhow::Result<()> {
    let work = crate::core::planning_work::load(store)?;
    anyhow::ensure!(
        work == snapshot.work,
        "planning work records changed during migration"
    );
    anyhow::ensure!(
        workflow_matches(&target.workflow, &expected_workflow(snapshot)),
        "workflow, approval, or fingerprint changed during migration"
    );
    let (open, resolved, _) = items_io::load_store(store)?;
    let (expected_open, expected_resolved) =
        normalize_item_uids(snapshot.open_items.clone(), snapshot.resolved_items.clone())?;
    anyhow::ensure!(
        open == expected_open && resolved == expected_resolved,
        "planning item records changed during migration"
    );
    anyhow::ensure!(
        !store
            .root
            .join(crate::artifacts::planning_store::paths::WORK)
            .exists()
            && !store
                .root
                .join(crate::artifacts::planning_store::paths::WORKFLOW)
                .exists()
            && !store
                .root
                .join(crate::artifacts::planning_store::paths::OPEN_ITEMS)
                .exists()
            && !store
                .root
                .join(crate::artifacts::planning_store::paths::RESOLVED_ITEMS)
                .exists(),
        "migration staging tree contains an authoritative monolithic state file"
    );
    audit_tasks(store, target)
}
