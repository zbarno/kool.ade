use crate::{
    artifacts::planning_store::StoreError,
    core::{
        lifecycle::LifecycleStore,
        opportunity::{OpportunityStore, OpportunityV1},
    },
};

use super::{RunRecord, SelfImprovementStore, records::validate_run_record};

pub(super) fn bind_parent(
    store: &SelfImprovementStore<'_>,
    proposal: &OpportunityV1,
    child: &mut super::super::SelfImprovementRunV1,
) -> Result<(), StoreError> {
    let Some(parent) = linked_parent(store, proposal, &child.feature_uid)? else {
        child.parent_run_uid = None;
        child.chain_depth = 0;
        return Ok(());
    };
    validate_parent(store, &parent)?;
    if parent.chain_depth >= super::super::MAX_SELF_IMPROVEMENT_CHAIN_DEPTH {
        return Err(StoreError::MalformedState(format!(
            "self-improvement chain depth limit ({}) has been reached",
            super::super::MAX_SELF_IMPROVEMENT_CHAIN_DEPTH
        )));
    }
    child.parent_run_uid = Some(parent.run_uid);
    child.chain_depth = parent.chain_depth + 1;
    Ok(())
}

pub(super) fn validate_parent(
    store: &SelfImprovementStore<'_>,
    run: &super::super::SelfImprovementRunV1,
) -> Result<(), StoreError> {
    let mut current = run.clone();
    let mut seen = std::collections::BTreeSet::from([current.run_uid.clone()]);
    loop {
        if current.schema_version >= 3 {
            let opportunities = OpportunityStore::new(store.store);
            let (proposal, _) = opportunities.load(&current.opportunity_uid)?;
            let expected_parent = linked_parent(store, &proposal, &current.feature_uid)?;
            if expected_parent
                .as_ref()
                .map(|parent| parent.run_uid.as_str())
                != current.parent_run_uid.as_deref()
                || expected_parent
                    .as_ref()
                    .map_or(0, |parent| parent.chain_depth.saturating_add(1))
                    != current.chain_depth
            {
                return Err(StoreError::MalformedState(
                    "self-improvement lineage does not match its opportunity links".into(),
                ));
            }
        }
        let Some(parent_uid) = current.parent_run_uid.clone() else {
            break;
        };
        if !seen.insert(parent_uid.clone()) {
            return Err(StoreError::MalformedState(
                "self-improvement lineage contains a cycle".into(),
            ));
        }
        let path = super::records::run_path(&parent_uid)?;
        let (record, revision) = store.store.read_record::<RunRecord>(&path)?;
        validate_run_record(&record, revision, &parent_uid, &path)?;
        if record.data.feature_uid == run.feature_uid
            || record.data.chain_depth.checked_add(1) != Some(current.chain_depth)
        {
            return Err(StoreError::MalformedState(
                "self-improvement lineage does not match its parent run".into(),
            ));
        }
        current = record.data;
    }
    Ok(())
}

fn linked_parent(
    store: &SelfImprovementStore<'_>,
    proposal: &OpportunityV1,
    child_feature_uid: &str,
) -> Result<Option<super::super::SelfImprovementRunV1>, StoreError> {
    let lifecycle = LifecycleStore::new(store.store);
    let mut feature_uids = proposal.related_feature_uids.clone();
    for task_uid in &proposal.related_task_uids {
        match lifecycle.load_task(task_uid) {
            Ok(task) => feature_uids.push(task.feature_uid),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            }
            Err(error) => return Err(error),
        }
    }
    feature_uids.sort();
    feature_uids.dedup();
    let mut parents = Vec::new();
    for feature_uid in feature_uids {
        if feature_uid != child_feature_uid {
            let linked_runs = runs_for_feature(store, &feature_uid)?;
            if linked_runs.len() > 1 {
                return Err(StoreError::MalformedState(
                    "multiple self-improvement runs are linked to one feature".into(),
                ));
            }
            parents.extend(linked_runs);
        }
    }
    parents.sort_by(|left, right| {
        left.chain_depth
            .cmp(&right.chain_depth)
            .then_with(|| left.run_uid.cmp(&right.run_uid))
    });
    Ok(parents.pop())
}

fn runs_for_feature(
    store: &SelfImprovementStore<'_>,
    feature_uid: &str,
) -> Result<Vec<super::super::SelfImprovementRunV1>, StoreError> {
    const RUNS: &str = "state/self_improvements";
    let mut runs = Vec::new();
    for path in store.store.list_record_paths(RUNS)? {
        let Some(uid) = path
            .strip_prefix("state/self_improvements/")
            .and_then(|file| file.strip_suffix(".json"))
        else {
            continue;
        };
        let (record, revision) = store.store.read_record::<RunRecord>(&path)?;
        validate_run_record(&record, revision, uid, &path)?;
        if record.data.feature_uid == feature_uid {
            runs.push(record.data);
        }
    }
    Ok(runs)
}
