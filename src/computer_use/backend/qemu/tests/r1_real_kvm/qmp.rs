use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
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
