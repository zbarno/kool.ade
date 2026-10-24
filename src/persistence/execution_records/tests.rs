use std::fs;

#[path = "tests/telemetry.rs"]
mod telemetry;

use super::*;

fn record() -> ExecutionRecordV1 {
    ExecutionRecordV1 {
        schema_version: super::record::SCHEMA_VERSION,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        request: None,
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        target_id: "local".into(),
        controller_target_id: None,
        owner_instance_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        container_name: Some("koolade-exec-test".into()),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: None,
        container_id: None,
        command_id: None,
        command_started: false,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-policy-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(2_000),
        attempt: 1,
        state: ExecutionRecordStateV1::Preparing,
        event_cursor: 0,
        output_bytes: 0,
        started_at_unix_ms: 10,
        updated_at_unix_ms: 10,
        ended_at_unix_ms: None,
        exit_code: None,
        artifacts: Vec::new(),
        docker_usage: DockerResourceUsageV1::default(),
        phase_timings: ExecutionPhaseTimingsV1::default(),
        cleanup: ExecutionCleanupStateV1::Pending,
    }
}

#[path = "tests/journal.rs"]
mod journal;

#[path = "tests/owner_scan.rs"]
mod owner_scan;

#[path = "tests/record_store.rs"]
mod record_store;

#[path = "tests/record_validation.rs"]
mod record_validation;
