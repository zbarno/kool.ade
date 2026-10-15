use sha2::{Digest, Sha256};

/// Fingerprint the checked-in policy, request validator, sandbox and approved
/// image manifest so remote workers reject a different security contract.
pub fn policy_hash() -> String {
    let artifacts: &[(&str, &[u8])] = &[
        ("policy", include_bytes!("../policy.rs")),
        ("policy_accessors", include_bytes!("accessors.rs")),
        ("policy_restriction", include_bytes!("restrict.rs")),
        ("validation", include_bytes!("validation.rs")),
        ("request", include_bytes!("../contract/request.rs")),
        ("argv", include_bytes!("../contract/request/argv.rs")),
        ("ids", include_bytes!("../contract/ids.rs")),
        ("events", include_bytes!("../contract/events.rs")),
        ("result", include_bytes!("../contract/result.rs")),
        ("backend_contract", include_bytes!("../backend.rs")),
        ("sandbox", include_bytes!("../docker/sandbox.rs")),
        (
            "workspace_staging",
            include_bytes!("../docker/sandbox/staging.rs"),
        ),
        (
            "workspace_staging_archive",
            include_bytes!("../docker/sandbox/staging/archive.rs"),
        ),
        (
            "workspace_staging_capture",
            include_bytes!("../docker/sandbox/staging/archive/capture.rs"),
        ),
        (
            "workspace_staging_sync",
            include_bytes!("../docker/sandbox/staging/sync.rs"),
        ),
        (
            "workspace_staging_sync_unix",
            include_bytes!("../docker/sandbox/staging/sync/unix.rs"),
        ),
        (
            "workspace_staging_artifacts",
            include_bytes!("../docker/sandbox/staging/artifacts.rs"),
        ),
        ("engine", include_bytes!("../docker/engine.rs")),
        (
            "engine_endpoint",
            include_bytes!("../docker/engine/endpoint.rs"),
        ),
        (
            "engine_api",
            include_bytes!("../docker/engine/bollard_api.rs"),
        ),
        ("discovery", include_bytes!("../docker/discovery.rs")),
        (
            "prerequisites",
            include_bytes!("../docker/prerequisites.rs"),
        ),
        ("backend_core", include_bytes!("../docker/backend.rs")),
        (
            "workspace_resolver",
            include_bytes!("../docker/backend/workspace_resolver.rs"),
        ),
        (
            "workspace_lifecycle",
            include_bytes!("../docker/backend/lifecycle/workspace.rs"),
        ),
        (
            "workspace_prepare",
            include_bytes!("../docker/backend/lifecycle/workspace/prepare.rs"),
        ),
        (
            "workspace_start",
            include_bytes!("../docker/backend/lifecycle/workspace/start.rs"),
        ),
        (
            "workspace_probe",
            include_bytes!("../docker/backend/lifecycle/workspace/probe.rs"),
        ),
        (
            "workspace_cleanup",
            include_bytes!("../docker/backend/lifecycle/workspace/cleanup.rs"),
        ),
        (
            "execution_lifecycle",
            include_bytes!("../docker/backend/lifecycle.rs"),
        ),
        (
            "execute",
            include_bytes!("../docker/backend/lifecycle/execute.rs"),
        ),
        (
            "execute_cleanup",
            include_bytes!("../docker/backend/lifecycle/execute/cleanup.rs"),
        ),
        (
            "execute_state",
            include_bytes!("../docker/backend/lifecycle/execute/state.rs"),
        ),
        (
            "execute_finish",
            include_bytes!("../docker/backend/lifecycle/execute/finish.rs"),
        ),
        (
            "execute_sync",
            include_bytes!("../docker/backend/lifecycle/execute/sync.rs"),
        ),
        (
            "execute_sync_cancellation",
            include_bytes!("../docker/backend/lifecycle/execute/sync/cancellation.rs"),
        ),
        (
            "execute_stream",
            include_bytes!("../docker/backend/lifecycle/execute/stream.rs"),
        ),
        (
            "control",
            include_bytes!("../docker/backend/lifecycle/control.rs"),
        ),
        (
            "control_cancel",
            include_bytes!("../docker/backend/lifecycle/control/cancel.rs"),
        ),
        (
            "control_release",
            include_bytes!("../docker/backend/lifecycle/control/release.rs"),
        ),
        (
            "control_helpers",
            include_bytes!("../docker/backend/lifecycle/control/helpers.rs"),
        ),
        (
            "recovery",
            include_bytes!("../docker/backend/lifecycle/recovery.rs"),
        ),
        (
            "recovery_restore",
            include_bytes!("../docker/backend/lifecycle/recovery/restore.rs"),
        ),
        (
            "recovery_interrupt",
            include_bytes!("../docker/backend/lifecycle/recovery/interrupt.rs"),
        ),
        (
            "runtime_state",
            include_bytes!("../docker/backend/lifecycle/runtime.rs"),
        ),
        ("images", include_bytes!("../docker/images.rs")),
        (
            "image_lock",
            include_bytes!("../../../containers/images.lock.json"),
        ),
    ];
    let mut hash = Sha256::new();
    for (name, contents) in artifacts {
        let normalized = contents
            .iter()
            .copied()
            .filter(|byte| *byte != b'\r')
            .collect::<Vec<_>>();
        hash.update((*name).as_bytes());
        hash.update((normalized.len() as u64).to_be_bytes());
        hash.update(normalized);
    }
    format!("sha256:{:x}", hash.finalize())
}
