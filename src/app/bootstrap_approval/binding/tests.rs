use super::*;

#[test]
fn machine_and_principal_hashes_are_derived_and_domain_separated() {
    let first = MachineBinding::from_facts(Host::Linux, "machine-one", "1000", &"a".repeat(64), 10)
        .unwrap();
    let other_principal =
        MachineBinding::from_facts(Host::Linux, "machine-one", "1001", &"a".repeat(64), 10)
            .unwrap();
    let other_machine =
        MachineBinding::from_facts(Host::Linux, "machine-two", "1000", &"a".repeat(64), 10)
            .unwrap();
    assert_ne!(first.machine_hash, other_machine.machine_hash);
    assert_ne!(first.principal_hash, other_principal.principal_hash);
    first.validate(11).unwrap();
    assert!(first.validate(10 + SCAN_TTL_MS + 1).is_err());
}

#[test]
fn native_identity_binding_uses_the_current_host_principal_digest() {
    let first = HostIdentity::from_facts(Host::Windows, "11", "machine", "sid-one").unwrap();
    let other_user = HostIdentity::from_facts(Host::Windows, "11", "machine", "sid-two").unwrap();
    let binding = MachineBinding::from_host_identity(&first, &"a".repeat(64), 10).unwrap();
    let other_binding =
        MachineBinding::from_host_identity(&other_user, &"a".repeat(64), 10).unwrap();
    assert_eq!(binding.host, Host::Windows);
    assert_eq!(binding.machine_hash.as_str(), first.machine_fingerprint());
    assert_eq!(
        binding.principal_hash.as_str(),
        first.principal_fingerprint()
    );
    assert_ne!(binding.principal_hash, other_binding.principal_hash);
}

#[test]
fn endpoint_and_probe_hash_must_be_sha256() {
    let binding = MachineBinding::from_facts(Host::Linux, "id", "1", "not-a-hash", 10).unwrap();
    assert!(binding.validate(11).is_err());
}

#[test]
fn free_space_changes_revoke_consent_and_are_rechecked() {
    let binding =
        MachineBinding::from_facts(Host::Linux, "machine", "1000", &"a".repeat(64), 10).unwrap();
    let digest = binding.consent_digest().unwrap();
    let mut refreshed = binding.clone();
    refreshed.disk_available_bytes = Some(refreshed.disk_available_bytes.unwrap() - 1);
    assert_ne!(refreshed.consent_digest().unwrap(), digest);
    refreshed.validate(11).unwrap();

    refreshed.disk_available_bytes = Some(
        refreshed
            .minimum_disk_bytes
            .saturating_add(refreshed.estimated_storage_bytes)
            - 1,
    );
    assert!(refreshed.validate(11).is_err());
}

#[test]
fn unknown_capacity_is_limited_to_desktop_general_and_explicitly_bound() {
    let mut desktop =
        MachineBinding::from_facts(Host::MacOs, "machine", "501", &"a".repeat(64), 10).unwrap();
    desktop.disk_available_bytes = None;
    desktop.capacity_probe_required = true;
    desktop.estimated_pull_bytes = 8 * 1024 * 1024;
    desktop.estimated_storage_bytes = 16 * 1024 * 1024;
    let digest = desktop.consent_digest().unwrap();
    desktop.validate(11).unwrap();

    let mut measured = desktop.clone();
    measured.disk_available_bytes = Some(4 * 1024 * 1024 * 1024);
    measured.capacity_probe_required = false;
    measured.validate(11).unwrap();
    assert_ne!(measured.consent_digest().unwrap(), digest);

    let mut linux = desktop.clone();
    linux.host = Host::Linux;
    assert!(linux.validate(11).is_err());
    let mut other_profile = desktop;
    other_profile.image_profile = "rust".into();
    assert!(other_profile.validate(11).is_err());
}
