use super::*;

fn artifact(size_bytes: u64) -> ArtifactManifestEntryV1 {
    ArtifactManifestEntryV1 {
        relative_path: "result.json".into(),
        size_bytes,
        sha256: "a".repeat(64),
        media_type: "application/json".into(),
    }
}

fn result(artifacts: Vec<ArtifactManifestEntryV1>) -> ExecutionResultV1 {
    ExecutionResultV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        exit_code: Some(0),
        signal: None,
        timed_out: false,
        cancelled: false,
        metrics: ResourceMetricsV1 {
            cpu_millis: 0,
            peak_memory_bytes: 0,
            process_count: 0,
            disk_bytes: 0,
        },
        artifacts,
        failure: None,
    }
}

#[test]
fn artifact_manifest_enforces_entry_count_and_aggregate_byte_quotas() {
    assert!(result(vec![artifact(0); 256]).validate().is_ok());
    assert!(result(vec![artifact(0); 257]).validate().is_err());

    let aggregate_limit = 1024 * 1024 * 1024;
    assert!(
        result(vec![artifact(aggregate_limit), artifact(0)])
            .validate()
            .is_ok()
    );
    assert!(
        result(vec![artifact(aggregate_limit), artifact(1)])
            .validate()
            .is_err()
    );
}
