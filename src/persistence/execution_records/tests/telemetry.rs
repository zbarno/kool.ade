use super::*;

#[test]
fn execution_records_default_new_telemetry_fields_for_older_saved_records() {
    let mut legacy = serde_json::to_value(record()).unwrap();
    let object = legacy.as_object_mut().unwrap();
    object.remove("docker_usage");
    object.remove("phase_timings");

    let restored: ExecutionRecordV1 = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.docker_usage, DockerResourceUsageV1::default());
    assert_eq!(restored.phase_timings, ExecutionPhaseTimingsV1::default());
}

#[test]
fn legacy_completed_record_without_cleanup_timing_remains_valid() {
    let mut legacy = record();
    legacy.state = ExecutionRecordStateV1::Succeeded;
    legacy.updated_at_unix_ms = 11;
    legacy.ended_at_unix_ms = Some(11);
    legacy.exit_code = Some(0);
    legacy.cleanup = ExecutionCleanupStateV1::Complete;
    legacy.result = Some(crate::execution::contract::ExecutionResultV1 {
        schema_version: 1,
        execution_id: legacy.execution_id.clone(),
        exit_code: Some(0),
        signal: None,
        timed_out: false,
        cancelled: false,
        metrics: crate::execution::contract::ResourceMetricsV1 {
            cpu_millis: 0,
            peak_memory_bytes: 0,
            process_count: 0,
            disk_bytes: 0,
        },
        artifacts: Vec::new(),
        failure: None,
    });
    let mut saved = serde_json::to_value(legacy).unwrap();
    saved.as_object_mut().unwrap().remove("phase_timings");

    let restored: ExecutionRecordV1 = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.cleanup, ExecutionCleanupStateV1::Complete);
    assert_eq!(restored.phase_timings.cleanup_ms, None);
    restored.validate().unwrap();
}

#[test]
fn phase_timing_validation_rejects_unbounded_durations() {
    let mut record = record();
    record.phase_timings.image_preparation_ms = Some(24 * 60 * 60 * 1000 + 1);
    assert!(record.validate().is_err());
}

#[test]
fn docker_resource_usage_rejects_metrics_outside_bounded_limits() {
    let mut record = record();
    record.docker_usage.sample_count = 4097;
    assert!(record.validate().is_err());

    record.docker_usage.sample_count = 1;
    record.docker_usage.sampled_peak_memory_bytes = Some(64 * 1024 * 1024 * 1024 + 1);
    assert!(record.validate().is_err());
}
