use super::super::super::{
    journal::{RecordMutation, SyncOperation},
    merge,
};
use super::super::intent::now_ms;

pub(super) fn later_operations(
    operations: &mut [SyncOperation],
    path: &str,
    original_value: Option<&str>,
    rebased_value: Option<&str>,
) -> anyhow::Result<()> {
    let mut original_base = original_value.map(str::to_owned);
    let mut rebased_base = rebased_value.map(str::to_owned);
    for operation in operations {
        let Some(mutation) = operation
            .mutations
            .iter_mut()
            .find(|mutation| mutation.path == path)
        else {
            continue;
        };
        if mutation.base != original_base {
            break;
        }
        original_base = mutation.desired.clone();
        rebase_mutation(mutation, rebased_base.as_deref())?;
        rebased_base = mutation.desired.clone();
    }
    Ok(())
}

fn rebase_mutation(mutation: &mut RecordMutation, base: Option<&str>) -> anyhow::Result<()> {
    mutation.base = base.map(str::to_owned);
    let Some((uid, _)) = merge::record_json_uid(&mutation.path) else {
        return Ok(());
    };
    let base_revision = revision(base)?;
    mutation.expected_revision = Some(base_revision);
    let Some(desired) = mutation.desired.as_mut() else {
        return Ok(());
    };
    let mut record: serde_json::Value = serde_json::from_str(desired)?;
    anyhow::ensure!(
        record.get("uid").and_then(serde_json::Value::as_str) == Some(uid),
        "queued planning record identity does not match its path"
    );
    let object = record
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("queued planning record must be a JSON object"))?;
    object.insert(
        "revision".into(),
        serde_json::Value::from(base_revision.saturating_add(1)),
    );
    object.insert("updatedAtMs".into(), serde_json::Value::from(now_ms()));
    *desired = serde_json::to_string_pretty(&record)?;
    Ok(())
}

fn revision(value: Option<&str>) -> anyhow::Result<u64> {
    value
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()?
        .map(|record| {
            record
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("queued planning record revision is missing"))
        })
        .transpose()
        .map(|revision| revision.unwrap_or(0))
}
