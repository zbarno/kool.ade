use super::{CacheStageRequest, MESSAGE_LIMIT, SessionBinding, ToolResponse};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

pub(crate) fn stage_cache(
    config_path: &Path,
    binding: &SessionBinding,
    snapshot: crate::harness::resource_bridge::CacheSnapshot,
) -> anyhow::Result<()> {
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)?;
    let socket_path = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MCP config has no private parent directory"))?
        .join("execution.sock");
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(1200)))?;
    let target_arch = binding
        .target_arch
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Cache target architecture is unavailable"))?;
    let request = CacheStageRequest {
        session_id: binding.session_id.clone(),
        capability_id: binding.capability_id.clone(),
        task_id: binding.task_id.clone(),
        workspace_id: binding.workspace_id.clone(),
        access: binding.access.clone(),
        target_id: binding.target_id.clone(),
        target_arch,
        cache_ref: snapshot.cache_ref,
        manifest: snapshot.manifest,
        archive_bytes: snapshot.archive.len(),
    };
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;
    stream.write_all(&snapshot.archive)?;
    stream.flush()?;
    let mut line = Vec::new();
    BufReader::new(stream)
        .take((MESSAGE_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= MESSAGE_LIMIT,
        "Cache staging response exceeds the limit"
    );
    let response: ToolResponse = serde_json::from_slice(&line)?;
    anyhow::ensure!(!response.is_error, "{}", response.stderr);
    Ok(())
}
