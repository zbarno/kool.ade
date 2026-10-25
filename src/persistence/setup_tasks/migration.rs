use std::collections::BTreeMap;

use serde::Deserialize;

use super::{
    DependencyResolution, SetupDependencyV1, SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1,
    SetupTaskV1,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyStoreV1 {
    schema_version: u8,
    tasks: BTreeMap<String, LegacyTaskV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyTaskV1 {
    id: String,
    title: String,
    explanation: String,
    next_step: String,
    prerequisites: Vec<String>,
    state: LegacyState,
    observed_at_unix_ms: u64,
    verified_at_unix_ms: Option<u64>,
    events: Vec<LegacyEvent>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyState {
    NeedsAttention,
    BlockedByPrerequisite,
    Unverified,
    Unsupported,
    Verified,
    Interrupted,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyEvent {
    observed_at_unix_ms: u64,
    state: LegacyState,
}

pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<(SetupTaskStoreV1, bool)> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("setup task store has no schema version"))?;
    match version {
        1 => migrate_v1(serde_json::from_value(value)?),
        version if version == u64::from(super::model::FILE_SCHEMA_VERSION) => {
            let store: SetupTaskStoreV1 = serde_json::from_value(value)?;
            store.validate()?;
            Ok((store, false))
        }
        _ => anyhow::bail!("unsupported setup task store schema version {version}"),
    }
}

fn migrate_v1(legacy: LegacyStoreV1) -> anyhow::Result<(SetupTaskStoreV1, bool)> {
    anyhow::ensure!(
        legacy.schema_version == 1,
        "invalid legacy setup store schema"
    );
    anyhow::ensure!(
        legacy.tasks.len() <= super::MAX_TASKS,
        "legacy setup task store is oversized"
    );
    let mut store = SetupTaskStoreV1::default();
    for (key, old) in legacy.tasks {
        anyhow::ensure!(
            key == old.id && super::valid_id(&key),
            "invalid legacy setup task ID"
        );
        let legacy_source_id = match key.as_str() {
            "setup:docker" | "setup:provider" => Some(key.clone()),
            _ => None,
        };
        let id = match key.as_str() {
            "setup:docker" => "setup:legacy:docker".to_owned(),
            "setup:provider" => "setup:legacy:provider".to_owned(),
            _ => key,
        };

        let mut state = migrate_state(old.state);
        if state == SetupTaskStateV1::Verified {
            // V1 did not store the domain that justified this state.
            state = SetupTaskStateV1::NeedsVerification;
        }
        if legacy_source_id.is_some() {
            state = SetupTaskStateV1::NeedsVerification;
        }
        let mut prior_events = old.events;
        if prior_events.len() > super::MAX_EVENTS {
            prior_events.drain(..prior_events.len() - super::MAX_EVENTS);
        }
        let events = prior_events
            .into_iter()
            .map(|event| SetupTaskEventV1 {
                observed_at_unix_ms: event.observed_at_unix_ms,
                scan_generation: 0,
                state: migrate_state(event.state),
            })
            .collect();
        let dependencies = old
            .prerequisites
            .into_iter()
            .map(|prerequisite_id| SetupDependencyV1 {
                prerequisite_id,
                resolution: DependencyResolution::Pending,
            })
            .collect::<Vec<_>>();
        let blocked_by: Vec<String> = dependencies
            .iter()
            .map(|dependency| dependency.prerequisite_id.clone())
            .collect();
        if !blocked_by.is_empty() && state != SetupTaskStateV1::Unsupported {
            state = SetupTaskStateV1::BlockedByPrerequisite;
        } else if state == SetupTaskStateV1::BlockedByPrerequisite {
            state = SetupTaskStateV1::NeedsVerification;
        }
        let task = SetupTaskV1 {
            schema_version: 1,
            id: id.clone(),
            title: old.title,
            explanation: old.explanation,
            next_step: old.next_step,
            dependencies,
            blocked_by,
            state,
            created_at_unix_ms: old.observed_at_unix_ms,
            observed_at_unix_ms: old.observed_at_unix_ms,
            scan_generation: 0,
            verified_at_unix_ms: old.verified_at_unix_ms,
            verified_result: None,
            attempts: Vec::new(),
            events,
            legacy_source_id,
            superseded_by: Vec::new(),
        };
        anyhow::ensure!(
            store.tasks.insert(id, task).is_none(),
            "legacy setup task IDs collide after migration"
        );
    }
    store.validate()?;
    Ok((store, true))
}

fn migrate_state(state: LegacyState) -> SetupTaskStateV1 {
    match state {
        LegacyState::NeedsAttention => SetupTaskStateV1::Detected,
        LegacyState::BlockedByPrerequisite => SetupTaskStateV1::BlockedByPrerequisite,
        LegacyState::Unverified => SetupTaskStateV1::NeedsVerification,
        LegacyState::Unsupported => SetupTaskStateV1::Unsupported,
        LegacyState::Verified => SetupTaskStateV1::Verified,
        LegacyState::Interrupted => SetupTaskStateV1::Interrupted,
    }
}
