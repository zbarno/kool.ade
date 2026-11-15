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

/// Require the complete live PCI graph to match the small q35 device set used
/// by the R1 guest. Unknown PCI devices can represent host passthrough and fail
/// closed even if they are not network-class devices.
pub(super) fn assert_approved_pci_devices(path: &Path) -> Result<(), String> {
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
    validate_approved_pci_devices(buses)
}

/// Require the live character-device graph to expose only the approved
/// session-local QMP/HMP/control sockets and diagnostic serial endpoint. This
/// catches unexpected host serial, pipe, file, or socket backends added by QEMU.
pub(super) fn assert_only_session_chardevs(
    path: &Path,
    session_directory: &Path,
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
    let chardevs = execute(&mut reader, "query-chardev", 2)?;
    let chardevs = chardevs
        .get("return")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "QMP query-chardev returned no character-device graph".to_owned())?;
    validate_session_chardevs(chardevs, session_directory)
}

fn validate_session_chardevs(
    chardevs: &[serde_json::Value],
    session_directory: &Path,
) -> Result<(), String> {
    let expected = BTreeSet::from([
        (
            "compat_monitor1",
            format!(
                "unix:{}/qemu-qmp.sock,server=on",
                session_directory.display()
            ),
        ),
        (
            "guestcontrol",
            format!(
                "unix:{}/guest-control.sock,server=on",
                session_directory.display()
            ),
        ),
        (
            "compat_monitor0",
            format!(
                "disconnected:unix:{}/qemu-monitor.sock,server=on",
                session_directory.display()
            ),
        ),
        ("serial0", "file".to_owned()),
    ]);
    let actual = chardevs
        .iter()
        .map(|entry| {
            Ok((
                entry
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "QMP character device has no label".to_owned())?,
                entry
                    .get("filename")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "QMP character device has no filename".to_owned())?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    if actual != expected {
        return Err(format!(
            "QEMU character-device graph has unexpected or missing host endpoints: {actual:?}"
        ));
    }
    Ok(())
}

fn validate_approved_pci_devices(buses: &[serde_json::Value]) -> Result<(), String> {
    let actual = pci_fingerprints(buses)?;
    let mut expected = vec![
        (0x0100, 0x1af4, 0x1001), // virtio block device
        (0x0106, 0x8086, 0x2922), // q35 SATA controller
        (0x0300, 0x1af4, 0x1050), // virtio VGA
        (0x0600, 0x8086, 0x29c0), // q35 host bridge
        (0x0601, 0x8086, 0x2918), // q35 ISA bridge
        (0x0780, 0x1af4, 0x1003), // virtio serial
        (0x0c05, 0x8086, 0x2930), // q35 SMBus
    ];
    expected.sort_unstable();
    if actual != expected {
        return Err(format!(
            "QEMU PCI graph has unexpected or missing devices: {actual:?}"
        ));
    }
    Ok(())
}

fn pci_fingerprints(buses: &[serde_json::Value]) -> Result<Vec<(u64, u64, u64)>, String> {
    fn visit_devices(
        devices: &[serde_json::Value],
        found: &mut Vec<(u64, u64, u64)>,
    ) -> Result<(), String> {
        for device in devices {
            let fingerprint = (
                device
                    .pointer("/class_info/class")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "QMP PCI device has no class code".to_owned())?,
                device
                    .pointer("/id/vendor")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "QMP PCI device has no vendor ID".to_owned())?,
                device
                    .pointer("/id/device")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "QMP PCI device has no device ID".to_owned())?,
            );
            found.push(fingerprint);
            if let Some(nested) = device
                .pointer("/pci_bridge/devices")
                .and_then(serde_json::Value::as_array)
            {
                visit_devices(nested, found)?;
            }
        }
        Ok(())
    }

    let mut found = Vec::new();
    for bus in buses {
        let devices = bus
            .get("devices")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "QMP PCI bus has no device list".to_owned())?;
        visit_devices(devices, &mut found)?;
    }
    found.sort_unstable();
    Ok(found)
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
mod pci_tests {
    use super::*;

    fn bus_with(devices: &[(u64, u64, u64)]) -> serde_json::Value {
        serde_json::json!({
            "devices": devices
                .iter()
                .map(|(class, vendor, device)| serde_json::json!({
                    "class_info": {"class": class},
                    "id": {"vendor": vendor, "device": device}
                }))
                .collect::<Vec<_>>()
        })
    }

    const R1_Q35_DEVICES: &[(u64, u64, u64)] = &[
        (0x0100, 0x1af4, 0x1001),
        (0x0106, 0x8086, 0x2922),
        (0x0300, 0x1af4, 0x1050),
        (0x0600, 0x8086, 0x29c0),
        (0x0601, 0x8086, 0x2918),
        (0x0780, 0x1af4, 0x1003),
        (0x0c05, 0x8086, 0x2930),
    ];

    #[test]
    fn live_pci_graph_accepts_only_the_r1_q35_device_set() {
        assert!(validate_approved_pci_devices(&[bus_with(R1_Q35_DEVICES)]).is_ok());
    }

    #[test]
    fn live_pci_graph_rejects_unexpected_host_passthrough_devices() {
        let mut devices = R1_Q35_DEVICES.to_vec();
        devices.push((0x0300, 0x10de, 0x1c82));
        assert!(validate_approved_pci_devices(&[bus_with(&devices)]).is_err());
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

    #[test]
    fn character_device_graph_allows_only_session_scoped_endpoints() {
        let directory = PathBuf::from("/private/session");
        let graph = [
            (
                "compat_monitor1",
                "unix:/private/session/qemu-qmp.sock,server=on",
            ),
            (
                "guestcontrol",
                "unix:/private/session/guest-control.sock,server=on",
            ),
            (
                "compat_monitor0",
                "disconnected:unix:/private/session/qemu-monitor.sock,server=on",
            ),
            ("serial0", "file"),
        ]
        .map(|(label, filename)| serde_json::json!({"label": label, "filename": filename}));
        assert!(validate_session_chardevs(&graph, &directory).is_ok());
    }

    #[test]
    fn character_device_graph_rejects_unapproved_host_endpoints() {
        let directory = PathBuf::from("/private/session");
        let graph = [
            (
                "compat_monitor1",
                "unix:/private/session/qemu-qmp.sock,server=on",
            ),
            (
                "guestcontrol",
                "unix:/private/session/guest-control.sock,server=on",
            ),
            (
                "compat_monitor0",
                "disconnected:unix:/private/session/qemu-monitor.sock,server=on",
            ),
            ("serial0", "file"),
        ]
        .map(|(label, filename)| serde_json::json!({"label": label, "filename": filename}));
        let graph = [
            graph[0].clone(),
            graph[1].clone(),
            graph[2].clone(),
            graph[3].clone(),
            serde_json::json!({
                "label": "extra-host-serial",
                "filename": "file:/etc/shadow",
                "frontend-open": true
            }),
        ];
        assert!(validate_session_chardevs(&graph, &directory).is_err());
    }
}
