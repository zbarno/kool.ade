use std::{
    io::{BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

const NETDEV_ID: &str = "r1-unapproved-isolated-hub";
const DEVICE_ID: &str = "r1-unapproved-virtual-nic";

/// Prove the live graph detector rejects an injected network device while
/// keeping it connected only to an isolated QEMU hub with no host backend.
pub fn assert_unapproved_network_hotplug_is_denied(qmp_path: &Path) -> Result<(), String> {
    let stream = UnixStream::connect(qmp_path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let greeting = super::read_json_line(&mut reader)?;
    if greeting.get("QMP").is_none() {
        return Err("QEMU did not provide a QMP greeting for network-negative test".into());
    }
    super::execute(&mut reader, "qmp_capabilities", 1)?;
    execute_arguments(
        &mut reader,
        "netdev_add",
        serde_json::json!({"type": "hubport", "id": NETDEV_ID, "hubid": 0}),
        2,
    )?;
    let denial = execute_arguments(
        &mut reader,
        "device_add",
        serde_json::json!({
            "driver": "virtio-net-pci",
            "id": DEVICE_ID,
            "netdev": NETDEV_ID
        }),
        3,
    )
    .err()
    .ok_or_else(|| {
        "QEMU accepted a network device on the offline guest's PCIe root bus".to_owned()
    })?;
    if !denial.contains("does not support hotplugging") {
        let _ = execute_arguments(
            &mut reader,
            "netdev_del",
            serde_json::json!({"id": NETDEV_ID}),
            4,
        );
        return Err(format!(
            "network hotplug failed for an unexpected reason: {denial}"
        ));
    }
    let remove_netdev = execute_arguments(
        &mut reader,
        "netdev_del",
        serde_json::json!({"id": NETDEV_ID}),
        6,
    );
    remove_netdev?;
    drop(reader);
    super::assert_no_network_device(qmp_path)
        .map_err(|error| format!("QEMU network graph changed after denied hotplug: {error}"))?;
    super::assert_approved_pci_devices(qmp_path)
        .map_err(|error| format!("QEMU PCI graph did not recover after test cleanup: {error}"))
}

fn execute_arguments(
    reader: &mut BufReader<UnixStream>,
    command: &str,
    arguments: serde_json::Value,
    id: u8,
) -> Result<serde_json::Value, String> {
    let request = serde_json::json!({"execute": command, "arguments": arguments, "id": id});
    serde_json::to_writer(&mut *reader.get_mut(), &request)
        .map_err(|error| format!("encode QMP {command}: {error}"))?;
    reader
        .get_mut()
        .write_all(b"\r\n")
        .map_err(|error| error.to_string())?;
    for _ in 0..64 {
        let response = super::read_json_line(reader)?;
        if response.get("id").and_then(serde_json::Value::as_u64) == Some(id as u64) {
            if let Some(error) = response.get("error") {
                return Err(format!("QMP {command} failed: {error}"));
            }
            return Ok(response);
        }
    }
    Err(format!("QMP {command} response was not received"))
}
