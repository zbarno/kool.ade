use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

/// Query the live QEMU PCI graph and fail if the disabled-network VM has any
/// network-class device. This checks the device graph after QEMU has applied
/// its machine defaults, beyond the unit-level command-line assertion.
pub(super) fn assert_no_network_device(path: &Path) -> Result<(), String> {
    let stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let greeting = read_json_line(&mut reader)?;
    if greeting.get("QMP").is_none() {
        return Err("QEMU did not provide a QMP greeting".into());
    }

    execute(&mut reader, "qmp_capabilities", 1)?;
    let pci = execute(&mut reader, "query-pci", 2)?;
    let buses = pci
        .get("return")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "QMP query-pci returned no device graph".to_owned())?;
    if buses.iter().any(contains_network_device) {
        return Err("disabled-network VM exposes a PCI network device".into());
    }
    Ok(())
}

/// Check the live QEMU block graph exposes only this session's disposable
/// overlay and its signed, read-only base image. This catches unexpected host
/// files attached through QEMU even when command construction looked correct.
pub(super) fn assert_only_approved_block_files(
    path: &Path,
    overlay_path: &Path,
    base_image_path: &Path,
) -> Result<(), String> {
    let stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let greeting = read_json_line(&mut reader)?;
    if greeting.get("QMP").is_none() {
        return Err("QEMU did not provide a QMP greeting".into());
    }
    execute(&mut reader, "qmp_capabilities", 1)?;
    let blocks = execute(&mut reader, "query-block", 2)?;
    let blocks = blocks
        .get("return")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "QMP query-block returned no block graph".to_owned())?;
    if blocks.len() != 1 {
        return Err(format!(
            "expected one disposable guest block device, found {}",
            blocks.len()
        ));
    }

    validate_block_paths(
        &blocks[0],
        &BTreeSet::from([overlay_path.to_path_buf(), base_image_path.to_path_buf()]),
    )
}

fn validate_block_paths(
    block: &serde_json::Value,
    allowed: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    collect_block_paths(block, &mut paths);
    if paths != *allowed {
        return Err(format!(
            "QEMU block graph contains unexpected or missing host file paths: {paths:?}"
        ));
    }
    Ok(())
}

fn collect_block_paths(value: &serde_json::Value, paths: &mut BTreeSet<PathBuf>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "file" | "filename" | "backing_file")
                    && let Some(path) = value.as_str()
                {
                    paths.insert(PathBuf::from(path));
                }
                collect_block_paths(value, paths);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_block_paths(value, paths);
            }
        }
        _ => {}
    }
}

fn execute(
    reader: &mut BufReader<UnixStream>,
    command: &str,
    id: u8,
) -> Result<serde_json::Value, String> {
    let request = format!("{{\"execute\":\"{command}\",\"id\":{id}}}\r\n");
    reader
        .get_mut()
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    for _ in 0..64 {
        let response = read_json_line(reader)?;
        if response.get("id").and_then(serde_json::Value::as_u64) == Some(id as u64) {
            if let Some(error) = response.get("error") {
                return Err(format!("QMP {command} failed: {error}"));
            }
            return Ok(response);
        }
    }
    Err(format!("QMP {command} response was not received"))
}

fn read_json_line(reader: &mut BufReader<UnixStream>) -> Result<serde_json::Value, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if line.is_empty() {
        return Err("QMP closed before returning a response".into());
    }
    serde_json::from_str(&line).map_err(|error| format!("invalid QMP JSON: {error}"))
}

fn contains_network_device(value: &serde_json::Value) -> bool {
    if value
        .get("class_info")
        .and_then(|class| class.get("class"))
        .and_then(serde_json::Value::as_u64)
        == Some(0x0200)
    {
        return true;
    }
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_network_device),
        serde_json::Value::Object(values) => values.values().any(contains_network_device),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_graph_allows_only_the_session_overlay_and_signed_base_image() {
        let overlay = PathBuf::from("/private/session/overlay.qcow2");
        let image = PathBuf::from("/private/cache/image.qcow2");
        let allowed = BTreeSet::from([overlay.clone(), image.clone()]);
        let graph = serde_json::json!({
            "inserted": {
                "file": overlay,
                "image": {"filename": "/private/session/overlay.qcow2"},
                "backing_file": image,
            }
        });
        assert!(validate_block_paths(&graph, &allowed).is_ok());
    }

    #[test]
    fn block_graph_rejects_an_unapproved_host_file() {
        let overlay = PathBuf::from("/private/session/overlay.qcow2");
        let image = PathBuf::from("/private/cache/image.qcow2");
        let allowed = BTreeSet::from([overlay.clone(), image.clone()]);
        let graph = serde_json::json!({
            "inserted": {
                "file": overlay,
                "backing_file": image,
                "host_device": {"filename": "/dev/sda"},
            }
        });
        assert!(validate_block_paths(&graph, &allowed).is_err());
    }
}
