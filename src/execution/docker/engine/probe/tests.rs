use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::{DockerApi, EngineFuture, EngineSnapshot},
};

use super::{image_digests_with_retry, inspect_with_retry};

struct TransientApi {
    inspect_failures: AtomicUsize,
    inspect_calls: AtomicUsize,
    inventory_failures: AtomicUsize,
    inventory_calls: AtomicUsize,
    retryable: bool,
}

impl DockerApi for TransientApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        self.inspect_calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            if consume_failure(&self.inspect_failures) {
                Err(failure(self.retryable))
            } else {
                Ok(snapshot())
            }
        })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        self.inventory_calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            if consume_failure(&self.inventory_failures) {
                Err(failure(self.retryable))
            } else {
                Ok(vec!["sha256:verified".into()])
            }
        })
    }

    fn ensure_image<'a>(
        &'a self,
        _image: &'a crate::execution::docker::images::SelectedImage,
        _expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<
        'a,
        Result<crate::execution::docker::images::ImageObservation, PrerequisiteFailure>,
    > {
        Box::pin(async { unreachable!("retry helper tests do not prepare images") })
    }
}

#[test]
fn health_and_inventory_probes_retry_transient_failures() {
    let api = Arc::new(TransientApi {
        inspect_failures: AtomicUsize::new(1),
        inspect_calls: AtomicUsize::new(0),
        inventory_failures: AtomicUsize::new(2),
        inventory_calls: AtomicUsize::new(0),
        retryable: true,
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    assert_eq!(
        runtime
            .block_on(inspect_with_retry(api.as_ref(), Duration::from_secs(1)))
            .unwrap()
            .daemon_id,
        "synthetic-daemon"
    );
    assert_eq!(
        runtime
            .block_on(image_digests_with_retry(
                api.as_ref(),
                Duration::from_secs(1)
            ))
            .unwrap(),
        ["sha256:verified"]
    );
    assert_eq!(api.inspect_calls.load(Ordering::Relaxed), 2);
    assert_eq!(api.inventory_calls.load(Ordering::Relaxed), 3);
}

#[test]
fn non_retryable_probe_failures_return_without_retrying() {
    let api = Arc::new(TransientApi {
        inspect_failures: AtomicUsize::new(3),
        inspect_calls: AtomicUsize::new(0),
        inventory_failures: AtomicUsize::new(3),
        inventory_calls: AtomicUsize::new(0),
        retryable: false,
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    assert!(
        runtime
            .block_on(inspect_with_retry(api.as_ref(), Duration::from_secs(1)))
            .is_err()
    );
    assert_eq!(api.inspect_calls.load(Ordering::Relaxed), 1);
    assert!(
        runtime
            .block_on(image_digests_with_retry(
                api.as_ref(),
                Duration::from_secs(1)
            ))
            .is_err()
    );
    assert_eq!(api.inventory_calls.load(Ordering::Relaxed), 1);
}

fn consume_failure(failures: &AtomicUsize) -> bool {
    failures
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            if count > 0 { Some(count - 1) } else { None }
        })
        .is_ok()
}

fn failure(retryable: bool) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code: PrerequisiteCode::DaemonStopped,
        message: "synthetic transient failure".into(),
        retryable,
    }
}

fn snapshot() -> EngineSnapshot {
    EngineSnapshot {
        daemon_id: "synthetic-daemon".into(),
        daemon_version: "29.9.0".into(),
        api_version: "1.53".into(),
        minimum_api_version: "1.41".into(),
        os_type: "linux".into(),
        architecture: "amd64".into(),
        security_options: vec!["name=rootless".into(), "name=seccomp".into()],
        rootless: true,
        native_linux: true,
        docker_desktop: false,
        memory_limit: true,
        cpu_quota: true,
        pids_limit: true,
        cgroup_version: Some("2".into()),
        cgroup_driver: Some("systemd".into()),
        seccomp_enabled: true,
        docker_storage_accessible: true,
        disk_available_bytes: Some(8 * 1024 * 1024 * 1024),
        minimum_disk_bytes: 1024,
    }
}
