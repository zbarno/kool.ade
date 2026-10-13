use super::*;
use crate::execution::contract::{ArtifactManifestEntryV1, ExecutionResultV1, ResourceMetricsV1};
use sha2::{Digest, Sha256};

fn result_manifest(bytes: &[u8]) -> TransferManifestV1 {
    TransferManifestV1::results(
        "worker:build-1",
        "controller:desktop-1",
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
            artifacts: vec![ArtifactManifestEntryV1 {
                relative_path: "result.bin".into(),
                size_bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                media_type: "application/octet-stream".into(),
            }],
            failure: None,
        },
    )
    .unwrap()
}

#[test]
fn streams_result_files_as_bounded_sha_checked_chunks() {
    let source = tempfile::tempdir().unwrap();
    let bytes = vec![0x6a; MAX_TRANSFER_CHUNK_BYTES * 2 + 5];
    std::fs::write(source.path().join("result.bin"), &bytes).unwrap();
    let manifest = result_manifest(&bytes);
    let mut chunks = Vec::new();
    stream_file_chunks(&manifest, source.path(), "result.bin", |chunk| {
        chunks.push(chunk.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(chunks.len(), 3);
    assert_eq!(
        chunks.iter().map(|chunk| chunk.bytes.len()).sum::<usize>(),
        bytes.len()
    );
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.bytes.len() <= MAX_TRANSFER_CHUNK_BYTES)
    );
    assert_eq!(chunks[2].offset, (MAX_TRANSFER_CHUNK_BYTES * 2) as u64);
}

#[cfg(unix)]
#[test]
fn result_sender_rejects_symlinked_artifacts_and_parent_directories() {
    let source = tempfile::tempdir().unwrap();
    let outside = source.path().join("outside");
    std::fs::write(&outside, b"secret").unwrap();
    std::os::unix::fs::symlink(&outside, source.path().join("result.bin")).unwrap();
    let manifest = result_manifest(b"secret");
    assert!(stream_file_chunks(&manifest, source.path(), "result.bin", |_| Ok(())).is_err());
}
