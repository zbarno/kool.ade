use std::{collections::BTreeSet, fs, path::Path};

const HOST_SECRET_CANARY: &str = "KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY";

/// Verify the live QEMU process receives only its small, explicit environment.
/// The runner must set this canary before launching the opt-in test; its
/// presence in QEMU proves that the live process check is observing inheritance.
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
    validate_qemu_environment(&environment, session_directory)
}

fn validate_qemu_environment(environment: &[u8], session_directory: &Path) -> Result<(), String> {
    let expected = BTreeSet::from([
        "PATH=/usr/bin:/bin".to_owned(),
        "HOME=/nonexistent".to_owned(),
        "LC_ALL=C".to_owned(),
        format!("TMPDIR={}", session_directory.display()),
    ]);
    let actual = environment
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            std::str::from_utf8(entry)
                .map(str::to_owned)
                .map_err(|_| "QEMU environment contains a non-UTF-8 entry".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual != expected {
        return Err(format!(
            "QEMU environment differs from its explicit allowlist: {actual:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_environment_accepts_only_the_configured_allowlist() {
        let session =
            Path::new("/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001");
        let environment =
            b"PATH=/usr/bin:/bin\0HOME=/nonexistent\0LC_ALL=C\0TMPDIR=/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001\0";
        assert!(validate_qemu_environment(environment, session).is_ok());
    }

    #[test]
    fn qemu_environment_rejects_inherited_host_secrets() {
        let session =
            Path::new("/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001");
        let environment = b"PATH=/usr/bin:/bin\0HOME=/nonexistent\0LC_ALL=C\0TMPDIR=/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001\0KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY=synthetic-canary\0";
        assert!(validate_qemu_environment(environment, session).is_err());
    }

    #[test]
    fn qemu_environment_rejects_other_inherited_configuration() {
        let session =
            Path::new("/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001");
        let environment = b"PATH=/usr/bin:/bin\0HOME=/nonexistent\0LC_ALL=C\0TMPDIR=/tmp/koolade/runtime/sessions/00000000-0000-4000-8000-000000000001\0DISPLAY=:0\0";
        assert!(validate_qemu_environment(environment, session).is_err());
    }
}
