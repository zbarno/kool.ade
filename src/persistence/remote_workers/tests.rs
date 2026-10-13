use super::*;

fn registration() -> RemoteWorkerRegistration {
    RemoteWorkerRegistration {
        schema_version: 1,
        worker_id: "worker-east".into(),
        ssh_host_alias: "build-host".into(),
        ssh_principal: "koolade".into(),
        host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        enrolled_at_unix_ms: 1_800_000_000_000,
        enabled: true,
        max_jobs: 1,
        supported_architectures: Vec::new(),
        trusted_image_digests: Vec::new(),
        resource_limit: None,
        last_probe: None,
        policy_revision: String::new(),
    }
}

#[test]
fn local_worker_registration_is_durable_and_cannot_be_silently_rebound() {
    let root = tempfile::tempdir().unwrap();
    let original = registration();
    register_at(root.path(), original.clone()).unwrap();
    assert_eq!(
        load_from(root.path(), &original.worker_id).unwrap(),
        Some(original.clone())
    );

    let mut same_identity = original.clone();
    same_identity.enrolled_at_unix_ms += 1;
    assert_eq!(register_at(root.path(), same_identity).unwrap(), original);

    let mut rebound = original.clone();
    rebound.host_key_fingerprint = format!("SHA256:{}", "B".repeat(43));
    assert!(register_at(root.path(), rebound).is_err());
}

#[test]
fn worker_registration_rejects_unsafe_identities_and_symlink_records() {
    let mut invalid = registration();
    invalid.ssh_principal = "koolade@attacker".into();
    assert!(invalid.validate().is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let path = registration_path(root.path(), "worker-east").unwrap();
        create_private_dir(path.parent().unwrap()).unwrap();
        let target = root.path().join("other.json");
        fs::write(&target, serde_json::to_vec(&registration()).unwrap()).unwrap();
        symlink(target, &path).unwrap();
        assert!(load_from(root.path(), "worker-east").is_err());
    }
}

#[test]
fn legacy_enrollment_defaults_to_one_job_and_is_not_schedulable_until_configured() {
    let mut value = serde_json::to_value(registration()).unwrap();
    for field in [
        "enabled",
        "maxJobs",
        "supportedArchitectures",
        "trustedImageDigests",
        "lastProbe",
        "policyRevision",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let migrated: RemoteWorkerRegistration = serde_json::from_value(value).unwrap();
    assert!(migrated.enabled);
    assert_eq!(migrated.max_jobs, 1);
    assert!(migrated.supported_architectures.is_empty());
    assert!(migrated.trusted_image_digests.is_empty());
    assert!(migrated.last_probe.is_none());
}

#[test]
fn legacy_health_probe_defaults_new_runtime_observations() {
    let mut value = serde_json::json!({
        "observedAtUnixMs": 1_800_000_000_000u64,
        "health": "healthy",
        "architecture": "amd64",
        "runningJobs": 0,
        "policyRevision": "sha256:policy"
    });
    for field in [
        "dockerVersion",
        "trustedImageDigests",
        "resourceCeiling",
        "diskAvailableBytes",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let probe: RemoteWorkerProbe = serde_json::from_value(value).unwrap();
    assert!(probe.docker_version.is_none());
    assert!(probe.trusted_image_digests.is_empty());
    assert!(probe.resource_ceiling.is_none());
    assert!(probe.disk_available_bytes.is_none());
}
