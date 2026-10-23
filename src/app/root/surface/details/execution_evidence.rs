pub(super) fn remote_telemetry(
    record: Option<&crate::persistence::execution_records::ExecutionRecordV1>,
    task_id: &str,
) -> (
    Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    Option<String>,
) {
    let record = record.map(|record| {
        (
            record.execution_id.as_str(),
            record.controller_target_id.as_deref(),
        )
    });
    remote_telemetry_with(
        record,
        task_id,
        crate::persistence::remote_execution_telemetry::load,
        crate::persistence::remote_execution_telemetry::latest_for_task,
    )
}

fn remote_telemetry_with(
    record: Option<(&str, Option<&str>)>,
    task_id: &str,
    mut load: impl FnMut(
        &str,
    ) -> anyhow::Result<
        Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    >,
    mut latest_for_task: impl FnMut(
        &str,
    ) -> anyhow::Result<
        Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    >,
) -> (
    Option<crate::persistence::remote_execution_telemetry::RemoteExecutionTelemetryV1>,
    Option<String>,
) {
    if let Some((execution_id, Some(worker_id))) =
        record.filter(|(_, target)| target.is_some_and(|target| target != "local"))
    {
        return match load(execution_id) {
            Ok(telemetry) => match bind_remote_telemetry(execution_id, worker_id, telemetry) {
                Ok(telemetry) => (telemetry, None),
                Err(error) => (None, Some(error.to_owned())),
            },
            Err(error) => (None, Some(error.to_string())),
        };
    }
    if !is_canonical_task_id(task_id) {
        return (None, None);
    }
    match latest_for_task(task_id) {
        Ok(telemetry) => (telemetry, None),
        Err(error) => (None, Some(error.to_string())),
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
            queue_wait_ms: None,
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
            execution_phase_timings: None,
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

    #[test]
    fn local_execution_record_falls_back_to_latest_remote_task_telemetry() {
        let task_id = "00000000-0000-4000-8000-000000000005";
        let remote = telemetry(
            "00000000-0000-4000-8000-000000000006",
            "00000000-0000-4000-8000-000000000007",
        );
        let mut latest_called = false;

        let result = remote_telemetry_with(
            Some(("00000000-0000-4000-8000-000000000008", Some("local"))),
            task_id,
            |_| panic!("local execution record must not load remote execution telemetry"),
            |queried_task_id| {
                latest_called = true;
                assert_eq!(queried_task_id, task_id);
                Ok(Some(remote.clone()))
            },
        );

        assert!(latest_called);
        assert_eq!(result, (Some(remote), None));
    }
}
