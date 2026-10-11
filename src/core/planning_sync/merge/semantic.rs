use serde_json::{Map, Value};

pub(super) fn semantic_fields(value: &Value) -> Value {
    let mut fields = value.clone();
    if let Some(object) = fields.as_object_mut() {
        object.remove("revision");
        object.remove("updatedAtMs");
    }
    fields
}

pub(super) fn decode_record(bytes: &[u8], path: &str, expected_uid: &str) -> anyhow::Result<Value> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| anyhow::anyhow!("record {path} is malformed: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("record {path} is not a JSON object"))?;
    anyhow::ensure!(
        object.get("uid").and_then(Value::as_str) == Some(expected_uid),
        "record {path} UID does not match its path"
    );
    anyhow::ensure!(
        object.get("schemaVersion").and_then(Value::as_u64) == Some(1),
        "record {path} has an incompatible schema version"
    );
    let created = object.get("createdAtMs").and_then(Value::as_u64);
    let updated = object.get("updatedAtMs").and_then(Value::as_u64);
    anyhow::ensure!(
        created.is_some() && updated.is_some() && created <= updated,
        "record {path} has invalid timestamps"
    );
    if let Some(data_uid) = object
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("uid"))
        .and_then(Value::as_str)
    {
        anyhow::ensure!(
            data_uid == expected_uid,
            "record {path} data UID does not match its path"
        );
    }
    revision(&value, path)?;
    Ok(value)
}

pub(super) fn validate_immutable_fields(
    base: &Value,
    local: &Value,
    remote: &Value,
) -> Result<(), String> {
    for field in ["uid", "schemaVersion", "createdAtMs"] {
        if local.get(field) != base.get(field) {
            return Err(field.to_owned());
        }
        if remote.get(field) != base.get(field) {
            return Err(field.to_owned());
        }
    }
    Ok(())
}

pub(super) fn revision(value: &Value, path: &str) -> anyhow::Result<u64> {
    value
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("record {path} has no valid revision"))
}

pub(super) fn optional_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

pub(super) fn merge_value(
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
    path: &str,
    conflicts: &mut Vec<String>,
) -> Option<Value> {
    if local == base {
        return remote.cloned();
    }
    if remote == base || local == remote {
        return local.cloned();
    }
    let (Some(Value::Object(base)), Some(Value::Object(local)), Some(Value::Object(remote))) =
        (base, local, remote)
    else {
        conflicts.push(if path.is_empty() {
            "$".into()
        } else {
            path.to_owned()
        });
        return None;
    };
    let keys = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .collect::<std::collections::BTreeSet<_>>();
    let mut merged = Map::new();
    for key in keys {
        let field = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        if let Some(value) = merge_value(
            base.get(key),
            local.get(key),
            remote.get(key),
            &field,
            conflicts,
        ) {
            merged.insert(key.clone(), value);
        }
    }
    Some(Value::Object(merged))
}
