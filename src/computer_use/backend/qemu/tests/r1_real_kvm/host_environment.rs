use std::{fs, path::Path};

const HOST_SECRET_CANARY: &str = "KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY";

/// Verify the live QEMU process does not inherit a synthetic host secret.
/// The runner must set this variable before launching the opt-in test.
pub(super) fn assert_host_secret_not_inherited(session_directory: &Path) -> Result<(), String> {
    if std::env::var_os(HOST_SECRET_CANARY).is_none() {
        return Err(format!("test runner did not set {HOST_SECRET_CANARY}"));
    }
    let owner_path = session_directory.join("owner.json");
    let owner: serde_json::Value = serde_json::from_slice(
        &fs::read(&owner_path).map_err(|error| format!("read QEMU owner record: {error}"))?,
    )
    .map_err(|error| format!("parse QEMU owner record: {error}"))?;
    let pid = owner
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "QEMU owner record has no process id".to_owned())?;
    let environment = fs::read(format!("/proc/{pid}/environ"))
        .map_err(|error| format!("read QEMU process environment: {error}"))?;
    let secret_key = format!("{HOST_SECRET_CANARY}=");
    if environment
        .split(|byte| *byte == 0)
        .any(|entry| entry.starts_with(secret_key.as_bytes()))
    {
        return Err("QEMU inherited the host secret canary".into());
    }
    Ok(())
}
