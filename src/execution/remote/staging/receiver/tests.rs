use super::*;
use crate::execution::contract::{ArtifactManifestEntryV1, ExecutionResultV1, ResourceMetricsV1};
use sha2::{Digest, Sha256};

fn result(bytes: &[u8], path: &str) -> ExecutionResultV1 {
    ExecutionResultV1 {
        schema_version: 1,
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        exit_code: Some(0),
        signal: None,
        timed_out: false,
        cancelled: false,
        metrics: ResourceMetricsV1 {
            cpu_millis: 1,
            peak_memory_bytes: 1,
            process_count: 1,
            disk_bytes: 0,
        },
        phase_timings: Default::default(),
        artifacts: vec![ArtifactManifestEntryV1 {
            relative_path: path.into(),
            size_bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            media_type: "application/octet-stream".into(),
        }],
        failure: None,
    }
}

fn transfer(bytes: &[u8], path: &str) -> TransferManifestV1 {
    TransferManifestV1::results(
        "worker:build-1",
        "controller:desktop-1",
        result(bytes, path),
    )
    .unwrap()
}

#[test]
fn verified_workspace_result_requires_result_payload_and_exact_workspace_identity() {
    let root = tempfile::tempdir().unwrap();
    let expected = WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000002".into(),
        task_id: "00000000-0000-4000-8000-000000000003".into(),
        source_sha: "a".repeat(40),
    };
    let manifest = transfer(b"result", "result.bin");
    let receiver = StagingReceiver::begin(
        root.path(),
        manifest,
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    assert!(receiver.finalize_workspace_result(&expected).is_err());
}

#[test]
fn transfer_is_durable_resumable_and_exact_replays_are_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"verified result";
    let manifest = transfer(bytes, "artifacts/result.bin");
    let chunk = TransferChunkV1::new(
        manifest.transfer_id.clone(),
        "artifacts/result.bin",
        0,
        bytes.to_vec(),
    )
    .unwrap();
    {
        let mut receiver = StagingReceiver::begin(
            root.path(),
            manifest.clone(),
            "worker:build-1",
            "controller:desktop-1",
        )
        .unwrap();
        receiver.accept_chunk(&chunk).unwrap();
        assert_eq!(receiver.received_chunk_count(), 1);
    }
    let mut resumed = StagingReceiver::begin(
        root.path(),
        manifest.clone(),
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    resumed.accept_chunk(&chunk).unwrap();
    let output = resumed.finalize().unwrap();
    assert_eq!(
        std::fs::read(output.join("artifacts/result.bin")).unwrap(),
        bytes
    );
    assert_eq!(resumed.finalize().unwrap(), output);
}

#[test]
fn rejects_wrong_peer_truncated_file_and_conflicting_chunk_replay() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"expected result";
    let manifest = transfer(bytes, "result.bin");
    assert!(
        StagingReceiver::begin(
            root.path(),
            manifest.clone(),
            "worker:other",
            "controller:desktop-1"
        )
        .is_err()
    );
    let mut receiver = StagingReceiver::begin(
        root.path(),
        manifest.clone(),
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    assert!(receiver.finalize().is_err());
    let chunk = TransferChunkV1::new(
        manifest.transfer_id.clone(),
        "result.bin",
        0,
        bytes.to_vec(),
    )
    .unwrap();
    receiver.accept_chunk(&chunk).unwrap();
    let conflict = TransferChunkV1::new(
        manifest.transfer_id.clone(),
        "result.bin",
        0,
        b"other result!!!".to_vec(),
    )
    .unwrap();
    assert!(receiver.accept_chunk(&conflict).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_staging_root_and_hostile_artifact_paths() {
    let parent = tempfile::tempdir().unwrap();
    let real = parent.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = parent.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let manifest = transfer(b"x", "safe.bin");
    assert!(
        StagingReceiver::begin(&link, manifest, "worker:build-1", "controller:desktop-1").is_err()
    );
    assert!(
        TransferManifestV1::results(
            "worker:build-1",
            "controller:desktop-1",
            result(b"x", "../escape")
        )
        .is_err()
    );
}

#[test]
fn rejects_truncated_or_tampered_durable_journal_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let bytes = b"durable";
    let manifest = transfer(bytes, "result.bin");
    let mut receiver = StagingReceiver::begin(
        root.path(),
        manifest.clone(),
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    let chunk = TransferChunkV1::new(
        manifest.transfer_id.clone(),
        "result.bin",
        0,
        bytes.to_vec(),
    )
    .unwrap();
    receiver.accept_chunk(&chunk).unwrap();
    let journal = root.path().join(&manifest.transfer_id).join("chunks.jsonl");
    let mut contents = std::fs::read(&journal).unwrap();
    contents.pop();
    std::fs::write(&journal, contents).unwrap();
    assert!(
        StagingReceiver::begin(
            root.path(),
            manifest,
            "worker:build-1",
            "controller:desktop-1"
        )
        .is_err()
    );
}

#[test]
fn chunk_rejects_wrong_digest_and_unaligned_offsets() {
    let mut chunk = TransferChunkV1::new(
        "00000000-0000-4000-8000-000000000001",
        "file",
        0,
        b"x".to_vec(),
    )
    .unwrap();
    chunk.sha256 = "0".repeat(64);
    assert!(chunk.validate().is_err());
    assert!(
        TransferChunkV1::new(
            "00000000-0000-4000-8000-000000000001",
            "file",
            1,
            b"x".to_vec()
        )
        .is_err()
    );
    assert!(
        TransferChunkV1::new(
            "00000000-0000-4000-8000-000000000001",
            "../escape",
            0,
            b"x".to_vec()
        )
        .is_err()
    );
}

#[test]
fn reassembles_out_of_order_chunks_only_after_whole_file_verification() {
    let root = tempfile::tempdir().unwrap();
    let bytes = vec![0x3c; super::super::MAX_TRANSFER_CHUNK_BYTES * 2 + 7];
    let manifest = super::super::TransferManifestV1::results(
        "worker:build-1",
        "controller:desktop-1",
        result(&bytes, "large.bin"),
    )
    .unwrap();
    let mut receiver = StagingReceiver::begin(
        root.path(),
        manifest.clone(),
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    for offset in [
        (super::super::MAX_TRANSFER_CHUNK_BYTES * 2) as u64,
        super::super::MAX_TRANSFER_CHUNK_BYTES as u64,
        0,
    ] {
        let end = (offset as usize + super::super::MAX_TRANSFER_CHUNK_BYTES).min(bytes.len());
        let chunk = TransferChunkV1::new(
            manifest.transfer_id.clone(),
            "large.bin",
            offset,
            bytes[offset as usize..end].to_vec(),
        )
        .unwrap();
        receiver.accept_chunk(&chunk).unwrap();
    }
    let output = receiver.finalize().unwrap();
    assert_eq!(std::fs::read(output.join("large.bin")).unwrap(), bytes);
}
