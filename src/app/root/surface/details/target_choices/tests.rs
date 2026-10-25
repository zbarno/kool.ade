use super::*;

fn local_status() -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: crate::execution::docker::environment::DockerEnvironmentState::NotInstalled,
        engine_ready: false,
        message: "Docker is not installed".into(),
        desktop_vm: false,
        daemon_version: None,
        architecture: None,
        rootless: None,
        cpu_limits: None,
        memory_limits: None,
        pids_limit: None,
        cgroups_supported: None,
        available_images: Vec::new(),
        disk_available_bytes: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: None,
    }
}

fn worker(enabled: bool, health: RemoteWorkerHealth) -> remote_workers::RemoteWorkerRegistration {
    remote_workers::RemoteWorkerRegistration {
        schema_version: 1,
        worker_id: if enabled {
            "healthy-worker"
        } else {
            "disabled-worker"
        }
        .into(),
        ssh_host_alias: "worker-host".into(),
        ssh_principal: "developer".into(),
        host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        enrolled_at_unix_ms: 1,
        enabled,
        max_jobs: 1,
        supported_architectures: vec!["amd64".into()],
        trusted_image_digests: Vec::new(),
        resource_limit: None,
        last_probe: Some(remote_workers::RemoteWorkerProbe {
            observed_at_unix_ms: 1,
            health,
            architecture: Some("amd64".into()),
            running_jobs: 0,
            policy_revision: "v1".into(),
            docker_version: None,
            trusted_image_digests: Vec::new(),
            resource_ceiling: None,
            disk_available_bytes: None,
            available_cpu_millis: Some(1_000),
            available_memory_bytes: Some(1024 * 1024 * 1024),
            cpu_pressure_some_avg10_basis_points: Some(0),
            load_millis: Some(0),
            gpu_available: Some(false),
            display_available: Some(false),
        }),
        policy_revision: "v1".into(),
    }
}

#[test]
fn selected_worker_missing_from_enrollment_is_visible_but_unavailable() {
    let selected = TargetPreference::Worker("worker-no-longer-enrolled".into());
    let choices = choices(&selected, None, &local_status(), &[], false, 1);
    let choice = choices
        .iter()
        .find(|choice| choice.preference == selected)
        .expect("persisted selection remains visible");
    assert!(!choice.available);
    assert!(choice.reason.contains("no longer enrolled"));
    assert!(choices.iter().any(|choice| {
        choice.preference == TargetPreference::Auto && choice.label == "Automatic (trusted only)"
    }));
}

#[test]
fn disabled_and_unhealthy_workers_are_explained_and_unavailable() {
    let selected = TargetPreference::Auto;
    let workers = [
        worker(false, RemoteWorkerHealth::Healthy),
        worker(true, RemoteWorkerHealth::Disconnected),
    ];
    let choices = choices(&selected, None, &local_status(), &workers, false, 1);
    assert!(choices.iter().any(|choice| {
        choice.label.contains("disabled-worker")
            && !choice.available
            && choice.reason.contains("disabled")
    }));
    assert!(choices.iter().any(|choice| {
        choice.label.contains("healthy-worker")
            && !choice.available
            && choice.reason.contains("disconnected")
    }));
    assert!(choices.iter().any(|choice| {
        choice.preference == TargetPreference::Local
            && !choice.available
            && choice.reason.contains("Docker is not installed")
    }));
}

#[test]
fn reachable_desktop_engine_remains_unavailable_for_local_tasks() {
    let mut desktop = local_status();
    desktop.engine_ready = true;
    desktop.desktop_vm = true;
    desktop.rootless = Some(true);
    let choices = choices(&TargetPreference::Local, None, &desktop, &[], false, 1);
    let local = choices
        .iter()
        .find(|choice| choice.preference == TargetPreference::Local)
        .unwrap();
    assert!(!local.available);
    assert!(
        local
            .reason
            .contains("verified native Linux rootless Engine")
    );
}

#[test]
fn stale_or_future_healthy_probe_is_unavailable_for_target_selection() {
    let selected = TargetPreference::Auto;
    let stale_worker = worker(true, RemoteWorkerHealth::Healthy);
    let stale_choices = choices(
        &selected,
        None,
        &local_status(),
        &[stale_worker],
        false,
        crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS + 2,
    );
    let choice = stale_choices
        .iter()
        .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
        .expect("enrolled worker is shown");
    assert!(!choice.available);
    assert!(choice.reason.contains("probe is stale"));

    let boundary_worker = worker(true, RemoteWorkerHealth::Healthy);
    let boundary_choices = choices(
        &selected,
        None,
        &local_status(),
        &[boundary_worker],
        false,
        crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS + 1,
    );
    assert!(boundary_choices.iter().any(|choice| {
        choice.preference == TargetPreference::Worker("healthy-worker".into()) && choice.available
    }));

    let future_probe = worker(true, RemoteWorkerHealth::Healthy);
    let future_choices = choices(&selected, None, &local_status(), &[future_probe], false, 0);
    let future_choice = future_choices
        .iter()
        .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
        .expect("enrolled worker is shown");
    assert!(!future_choice.available);
    assert!(future_choice.reason.contains("probe is stale"));

    let mut zero_probe = worker(true, RemoteWorkerHealth::Healthy);
    zero_probe
        .last_probe
        .as_mut()
        .expect("fixture has a probe")
        .observed_at_unix_ms = 0;
    let zero_choices = choices(&selected, None, &local_status(), &[zero_probe], false, 0);
    let zero_choice = zero_choices
        .iter()
        .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
        .expect("enrolled worker is shown");
    assert!(!zero_choice.available);
    assert!(zero_choice.reason.contains("probe is stale"));
}

#[test]
fn healthy_worker_without_current_resource_measurements_is_unavailable() {
    let mut worker = worker(true, RemoteWorkerHealth::Healthy);
    let probe = worker.last_probe.as_mut().unwrap();
    probe.available_memory_bytes = None;
    let choices = choices(
        &TargetPreference::Auto,
        None,
        &local_status(),
        &[worker],
        false,
        2,
    );
    let choice = choices
        .iter()
        .find(|choice| choice.preference == TargetPreference::Worker("healthy-worker".into()))
        .unwrap();
    assert!(!choice.available);
    assert!(choice.reason.contains("live CPU, memory, and load"));
}
