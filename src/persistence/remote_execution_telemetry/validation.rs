use super::{MAX_BYTES, MAX_MS, RemoteExecutionTelemetryV1};

pub(super) fn validate_id(id: &str) -> anyhow::Result<()> {
    let parsed = uuid::Uuid::parse_str(id)?;
    anyhow::ensure!(
        parsed.hyphenated().to_string() == id,
        "execution ID is invalid"
    );
    Ok(())
}

pub(super) fn validate(record: &RemoteExecutionTelemetryV1) -> anyhow::Result<()> {
    validate_id(&record.execution_id)?;
    anyhow::ensure!(
        matches!(record.schema_version, 1..=6)
            && [record.workspace_upload_bytes, record.result_download_bytes]
                .iter()
                .flatten()
                .all(|value| *value <= MAX_BYTES)
            && [record.workspace_upload_ms, record.result_download_ms]
                .iter()
                .flatten()
                .all(|value| *value <= MAX_MS)
            && record
                .worker_start_rpc_ms
                .is_none_or(|value| value <= MAX_MS)
            && record.execution_wall_ms.is_none_or(|value| value <= MAX_MS)
            && record
                .execution_cpu_millis
                .is_none_or(|value| value <= MAX_MS * 16)
            && record
                .execution_peak_memory_bytes
                .is_none_or(|value| value <= 64 * 1024 * 1024 * 1024),
        "remote execution telemetry is invalid"
    );
    validate_worker_id(&record.worker_id)?;
    if let Some(harness) = &record.harness {
        anyhow::ensure!(
            matches!(
                harness.as_str(),
                "codex" | "claude" | "antigravity" | "opencode" | "copilot" | "pi"
            ),
            "remote harness attribution is invalid"
        );
    }
    if let Some(model) = &record.requested_model {
        anyhow::ensure!(
            !model.is_empty()
                && model.len() <= 128
                && model
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-@+".contains(&byte)),
            "requested model attribution is invalid"
        );
    }
    if let Some(task_id) = &record.task_id {
        validate_id(task_id)?;
    }
    if let Some(digest) = &record.image_digest {
        anyhow::ensure!(
            digest.len() == 71
                && digest.starts_with("sha256:")
                && digest[7..].bytes().all(|b| b.is_ascii_hexdigit()),
            "image digest is invalid"
        );
    }
    Ok(())
}

pub(super) fn validate_worker_id(worker_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !worker_id.is_empty()
            && worker_id.len() <= 128
            && worker_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "worker identity is invalid"
    );
    Ok(())
}
