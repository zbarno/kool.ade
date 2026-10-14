use crate::{
    artifacts::planning_store::{RecordRevisionCheck, StoreError},
    core::opportunity::{OpportunityStatusV1, OpportunityV1},
};

use super::{
    OpportunityRecord, OpportunityStore, encode, now_unix_ms, opportunity_path, render_markdown,
};

pub(super) fn mark_planning(
    store: &OpportunityStore<'_>,
    uid: &str,
    feature_uid: &str,
    expected_record_revision: u64,
) -> Result<(OpportunityV1, u64), StoreError> {
    let (mut proposal, actual_revision) = store.load(uid)?;
    if actual_revision != expected_record_revision {
        return Err(StoreError::StaleRecordRevision {
            path: opportunity_path(uid)?,
            expected: expected_record_revision,
            actual: actual_revision,
        });
    }
    if proposal.status != OpportunityStatusV1::ApprovedToPlan
        || uuid::Uuid::parse_str(feature_uid).is_err()
    {
        return Err(StoreError::MalformedState(
            "only a pursued opportunity can enter planning with a valid feature identity".into(),
        ));
    }
    if !proposal
        .related_feature_uids
        .contains(&feature_uid.to_owned())
    {
        proposal.related_feature_uids.push(feature_uid.to_owned());
    }
    proposal.status = OpportunityStatusV1::Planning;
    proposal.revision = proposal
        .revision
        .checked_add(1)
        .ok_or_else(|| StoreError::MalformedState("opportunity revision overflow".into()))?;
    proposal.validate().map_err(StoreError::MalformedState)?;
    let path = opportunity_path(uid)?;
    let (record, _) = store.store.read_record::<OpportunityRecord>(&path)?;
    let now = now_unix_ms();
    let updated = OpportunityRecord {
        schema_version: 1,
        uid: uid.to_owned(),
        created_at_ms: record.created_at_ms,
        updated_at_ms: now,
        revision: actual_revision + 1,
        data: proposal.clone(),
        prompt_proposal: record.prompt_proposal,
    };
    let changes = vec![
        (path.clone(), encode(&updated)?),
        (
            format!("planning/opportunities/{uid}.md"),
            render_markdown(&proposal).into_bytes(),
        ),
    ];
    store.store.transaction_with_record_revisions(
        &changes,
        &[RecordRevisionCheck {
            path,
            expected_revision: actual_revision,
        }],
    )?;
    Ok((proposal, actual_revision + 1))
}
