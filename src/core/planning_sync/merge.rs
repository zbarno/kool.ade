use serde_json::Value;

use super::journal::RecordMutation;
mod item;
mod semantic;
pub(super) use item::record_markdown_uid;
use item::validate_item_content;
use semantic::{
    decode_record, merge_value, optional_u64, revision, semantic_fields, validate_immutable_fields,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MergeResult {
    Applied(Option<Vec<u8>>),
    AlreadyApplied,
    Conflict(Vec<String>),
}

pub(super) fn apply_mutation(
    mutation: &RecordMutation,
    current: Option<&[u8]>,
) -> anyhow::Result<MergeResult> {
    let base = mutation.base.as_deref().map(str::as_bytes);
    let desired = mutation.desired.as_deref().map(str::as_bytes);

    if !is_record_collection_path(&mutation.path) {
        return apply_opaque_mutation(mutation, current);
    }
    if record_markdown_uid(&mutation.path).is_some() {
        validate_record_file(&mutation.path, current)?;
        validate_mutation(mutation)?;
        return apply_opaque_mutation(mutation, current);
    }
    let Some((path_uid, _)) = record_json_uid(&mutation.path) else {
        anyhow::bail!(
            "record path {} is not a canonical managed-record file",
            mutation.path
        );
    };

    validate_mutation(mutation)?;
    let base_value = base
        .map(|bytes| decode_record(bytes, &mutation.path, path_uid))
        .transpose()?;
    let desired_value = desired
        .map(|bytes| decode_record(bytes, &mutation.path, path_uid))
        .transpose()?;
    let current_value = current
        .map(|bytes| decode_record(bytes, &mutation.path, path_uid))
        .transpose()?;

    if current == base {
        return Ok(MergeResult::Applied(desired.map(<[u8]>::to_vec)));
    }
    if current == desired {
        return Ok(MergeResult::AlreadyApplied);
    }
    let (Some(base_value), Some(desired_value), Some(current_value)) =
        (base_value, desired_value, current_value)
    else {
        return Ok(MergeResult::Conflict(vec![mutation.path.clone()]));
    };
    let mut fields = Vec::new();
    let base_fields = semantic_fields(&base_value);
    let desired_fields = semantic_fields(&desired_value);
    let current_fields = semantic_fields(&current_value);
    let merged = merge_value(
        Some(&base_fields),
        Some(&desired_fields),
        Some(&current_fields),
        "",
        &mut fields,
    );
    if !fields.is_empty() {
        fields.sort();
        fields.dedup();
        return Ok(MergeResult::Conflict(fields));
    }
    let Some(Value::Object(mut merged)) = merged else {
        return Ok(MergeResult::Conflict(vec![mutation.path.clone()]));
    };
    if let Err(field) = validate_immutable_fields(&base_value, &desired_value, &current_value) {
        return Ok(MergeResult::Conflict(vec![field]));
    }
    let local_revision = revision(&desired_value, &mutation.path)?;
    let remote_revision = revision(&current_value, &mutation.path)?;
    merged.insert(
        "revision".into(),
        Value::from(local_revision.max(remote_revision).saturating_add(1)),
    );
    let local_updated = optional_u64(&desired_value, "updatedAtMs");
    let remote_updated = optional_u64(&current_value, "updatedAtMs");
    if let Some(timestamp) = [local_updated, remote_updated].into_iter().flatten().max() {
        merged.insert("updatedAtMs".into(), Value::from(timestamp));
    }
    Ok(MergeResult::Applied(Some(serde_json::to_vec_pretty(
        &Value::Object(merged),
    )?)))
}

pub(super) fn validate_mutation(mutation: &RecordMutation) -> anyhow::Result<()> {
    if !is_record_collection_path(&mutation.path) {
        return Ok(());
    }
    if let Some(uid) = record_markdown_uid(&mutation.path) {
        anyhow::ensure!(
            mutation.record_id == uid,
            "record {} identity does not match its path",
            mutation.path
        );
        anyhow::ensure!(
            mutation.expected_revision.is_none(),
            "item content {} cannot carry a record revision",
            mutation.path
        );
        for content in [mutation.base.as_deref(), mutation.desired.as_deref()]
            .into_iter()
            .flatten()
        {
            validate_item_content(content.as_bytes(), uid)?;
        }
        return Ok(());
    }
    let Some((path_uid, _)) = record_json_uid(&mutation.path) else {
        anyhow::bail!(
            "record path {} is not a canonical managed-record file",
            mutation.path
        );
    };
    anyhow::ensure!(
        mutation.record_id == path_uid,
        "record {} identity does not match its path",
        mutation.path
    );
    let base = mutation
        .base
        .as_deref()
        .map(|bytes| decode_record(bytes.as_bytes(), &mutation.path, path_uid))
        .transpose()?;
    let desired = mutation
        .desired
        .as_deref()
        .map(|bytes| decode_record(bytes.as_bytes(), &mutation.path, path_uid))
        .transpose()?;
    if let Some(expected) = mutation.expected_revision {
        let actual = base
            .as_ref()
            .map(|value| revision(value, &mutation.path))
            .transpose()?
            .unwrap_or(0);
        anyhow::ensure!(
            actual == expected,
            "record {} base revision does not match the expected revision",
            mutation.path
        );
        if let Some(desired) = desired {
            let desired_revision = revision(&desired, &mutation.path)?;
            anyhow::ensure!(
                desired_revision > expected,
                "record {} desired revision does not advance",
                mutation.path
            );
        }
    }
    Ok(())
}

pub(super) fn is_record_collection_path(path: &str) -> bool {
    [
        "state/work",
        "state/workflow",
        "state/items",
        "state/tasks",
        "planning/items",
    ]
    .iter()
    .any(|collection| path.starts_with(&format!("{collection}/")))
}

pub(super) fn validate_record_file(path: &str, bytes: Option<&[u8]>) -> anyhow::Result<()> {
    if !is_record_collection_path(path) {
        return Ok(());
    }
    if let Some(uid) = record_markdown_uid(path) {
        if let Some(bytes) = bytes {
            validate_item_content(bytes, uid)?;
        }
        return Ok(());
    }
    let Some((uid, _)) = record_json_uid(path) else {
        anyhow::bail!("record path {path} is not a canonical managed-record file");
    };
    if let Some(bytes) = bytes {
        decode_record(bytes, path, uid)?;
    }
    Ok(())
}

pub(super) fn record_json_uid(path: &str) -> Option<(&str, &str)> {
    let (collection, filename) = path.rsplit_once('/')?;
    let allowed = matches!(
        collection,
        "state/work" | "state/workflow" | "state/items" | "state/tasks"
    );
    let uid = filename.strip_suffix(".json")?;
    let parsed = uuid::Uuid::parse_str(uid).ok()?;
    (allowed && parsed.hyphenated().to_string() == uid).then_some((uid, collection))
}

fn apply_opaque_mutation(
    mutation: &RecordMutation,
    current: Option<&[u8]>,
) -> anyhow::Result<MergeResult> {
    let base = mutation.base.as_deref().map(str::as_bytes);
    let desired = mutation.desired.as_deref().map(str::as_bytes);
    if current == base {
        return Ok(MergeResult::Applied(desired.map(<[u8]>::to_vec)));
    }
    if current == desired {
        return Ok(MergeResult::AlreadyApplied);
    }
    Ok(MergeResult::Conflict(vec![mutation.path.clone()]))
}
