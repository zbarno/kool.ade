pub(super) fn remote_telemetry(
    record: Option<&crate::persistence::execution_records::ExecutionRecordV1>,
    task_id: &str,
) -> (
    Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    Option<String>,
) {
    match record {
        Some(record) => {
            let Some(worker_id) = record
                .controller_target_id
                .as_deref()
                .filter(|target| *target != "local")
            else {
                return (None, None);
            };
            match crate::persistence::remote_execution_telemetry::load(&record.execution_id) {
                Ok(telemetry) => {
                    match bind_remote_telemetry(&record.execution_id, worker_id, telemetry) {
                        Ok(telemetry) => (telemetry, None),
                        Err(error) => (None, Some(error.to_owned())),
                    }
                }
                Err(error) => (None, Some(error.to_string())),
            }
        }
        None if is_canonical_task_id(task_id) => {
            match crate::persistence::remote_execution_telemetry::latest_for_task(task_id) {
                Ok(telemetry) => (telemetry, None),
                Err(error) => (None, Some(error.to_string())),
            }
        }
        None => (None, None),
    }
}

fn is_canonical_task_id(task_id: &str) -> bool {
    uuid::Uuid::parse_str(task_id).is_ok_and(|id| id.hyphenated().to_string() == task_id)
}

fn bind_remote_telemetry(
    execution_id: &str,
    worker_id: &str,
    telemetry: Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
) -> Result<
    Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    &'static str,
> {
    let Some(telemetry) = telemetry else {
        return Ok(None);
    };
    if telemetry.execution_id != execution_id || telemetry.worker_id != worker_id {
        return Err("Remote worker telemetry does not match the execution record");
    }
    Ok(Some(telemetry))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telemetry(
        execution_id: &str,
        worker_id: &str,
    ) -> crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1 {
        crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1 {
            schema_version: 3,
            execution_id: execution_id.into(),
            started_at_unix_ms: Some(1),
            task_id: Some("00000000-0000-4000-8000-000000000005".into()),
            worker_id: worker_id.into(),
            harness: None,
            requested_model: None,
            workspace_upload_bytes: None,
            workspace_upload_ms: None,
            result_download_bytes: None,
            result_download_ms: None,
            worker_start_rpc_ms: None,
            execution_wall_ms: None,
            execution_cpu_millis: None,
            execution_peak_memory_bytes: None,
            image_digest: None,
            failure_code: None,
        }
    }

    #[test]
    fn remote_telemetry_must_match_execution_and_worker_identity() {
        let execution = "00000000-0000-4000-8000-000000000001";
        let worker = "00000000-0000-4000-8000-000000000002";
        assert!(
            bind_remote_telemetry(execution, worker, Some(telemetry(execution, worker))).is_ok()
        );
        assert!(
            bind_remote_telemetry(
                execution,
                worker,
                Some(telemetry("00000000-0000-4000-8000-000000000003", worker))
            )
            .is_err()
        );
        assert!(
            bind_remote_telemetry(
                execution,
                worker,
                Some(telemetry(execution, "00000000-0000-4000-8000-000000000004"))
            )
            .is_err()
        );
    }
}
