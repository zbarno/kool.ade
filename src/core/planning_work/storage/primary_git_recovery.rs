//! One-time, fenced recovery for private work written before primary Git
//! intent fields were consistently mapped. This never grants Git approval.
use crate::artifacts::planning_store::{PlanningRoot, RecordRevisionCheck, StoreError, paths};

/// Recover only unmapped primary-repository fields. Preserve every original
/// record byte-for-byte in private recovery storage before changing the record.
/// An existing *valid* approval may be mapped; branch names alone cannot
/// authorize execution.
pub fn recover_unmapped_primary_git_intents<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> anyhow::Result<usize> {
    let store = repo.planning_store();
    let revision = store.revision()?;
    let mut records = store.with_consistent_read(|| super::read_record_files(&store))?;
    let mut changes = Vec::new();
    let mut checks = Vec::new();
    let mut recovered = 0;

    for record in &mut records {
        let item = &mut record.data;
        if item.git_intents.contains_key("root")
            || (item.git_intent.is_none()
                && item.source_branch.is_none()
                && item.destination_branch.is_none())
        {
            continue;
        }

        let approved = item.git_intent.as_ref().filter(|intent| {
            intent.repository_id == "root"
                && intent.validate().is_ok()
                && item.source_branch.as_deref() == Some(intent.source_ref.as_str())
                && item.destination_branch.as_deref() == Some(intent.destination_ref.as_str())
        }).cloned();

        // No approval may be reconstructed from branch names. Preserve those
        // untrusted values only in the backup, not in executable work state.
        if let Some(approved) = approved {
            item.git_intents.insert("root".to_owned(), approved);
        } else {
            item.git_intent = None;
            item.source_branch = None;
            item.destination_branch = None;
        }

        let path = format!("{}/{}.json", paths::WORK_RECORDS, record.uid);
        let backup = format!(
            "state/recovery/unmapped-primary-git-intent/{}-revision-{}.json",
            record.uid, record.revision
        );
        match store.read(&backup) {
            Err(StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => anyhow::bail!("Recovery backup already exists: {backup}"),
            Err(error) => return Err(error.into()),
        }
        let original = store.read(&path)?;
        let previous_revision = record.revision;
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Work record revision exhausted"))?;
        record.updated_at_ms = super::now_ms().max(record.created_at_ms);
        checks.push(RecordRevisionCheck {
            path: path.clone(),
            expected_revision: previous_revision,
        });
        changes.push((backup, original));
        changes.push((path, serde_json::to_vec_pretty(record)?));
        recovered += 1;
    }

    if recovered == 0 {
        return Ok(0);
    }

    // Refuse to repair only part of a malformed workspace. The transactional
    // write then saves the backups and corrected records together.
    let work = records.into_iter().map(|record| record.data).collect::<Vec<_>>();
    super::validate(&work)?;
    store.transaction_with_revision_and_record_revisions(
        &changes,
        Some(&revision),
        &checks,
    )?;
    Ok(recovered)
}
