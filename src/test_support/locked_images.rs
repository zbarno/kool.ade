//! Test fixtures that create ordinary app-approved locked-image receipts.
//!
//! These helpers prepare isolated Docker images through the same preview,
//! one-use approval, locked pull, and smoke-test path as the application. They
//! exist for tests that spawn the production binary, which cannot use
//! `cfg(test)` execution hooks. The Docker daemon and preloaded locked images
//! are external dependencies; setup runs one bounded smoke container per
//! profile and isolated application home.

use std::{
    path::Path,
    sync::{OnceLock, atomic::AtomicBool, mpsc},
};

use crate::{
    app::bootstrap_repair_policy::{Host, Operation, Proposal},
    execution::{
        docker::{
            engine::{DockerEndpoint, EngineDiscovery, HostPlatform},
            images::{ImageLock, RuntimeProfile, SelectedImage},
        },
        errors::{ExecutionError, ExecutionErrorKind},
    },
    persistence::setup_tasks::{
        AuthorizationBindingV1, ImagePreparationReceiptV1, SetupTaskTransitionV1,
        authorization_scope_digest,
    },
    test_support::KooladeHomeGuard,
};

const APPROVAL_ENV: &str = "KOOLADE_TEST_APPROVE_LOCKED_IMAGES";
static PREPARATION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub(crate) fn enabled() -> bool {
    std::env::var(APPROVAL_ENV).ok().as_deref() == Some("1")
}

/// Seed a production child process's isolated home using ordinary persisted
/// setup state. The fixture must call this before the child is started.
pub(crate) fn prepare_profile_for_home(home: &Path, profile: RuntimeProfile) -> anyhow::Result<()> {
    let _home = KooladeHomeGuard::set(home);
    let endpoint = DockerEndpoint::from_process_environment(host_platform())
        .map_err(|failure| anyhow::anyhow!(failure.message))?;
    let engine =
        EngineDiscovery::connect(endpoint).map_err(|failure| anyhow::anyhow!(failure.message))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let snapshot = engine
            .inspect_snapshot()
            .await
            .map_err(|failure| anyhow::anyhow!(failure.message))?;
        let image = ImageLock::bundled()
            .and_then(|lock| lock.select(profile, &snapshot.architecture))
            .map_err(|failure| anyhow::anyhow!(failure.message))?;
        let staging = crate::execution::docker::environment::app_staging_root()
            .map_err(|failure| anyhow::anyhow!(failure.message))?;
        prepare_approved_image(&engine, &staging, &image)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    })
}

pub(crate) async fn prepare_approved_image(
    engine: &EngineDiscovery,
    staging_root: &Path,
    image: &SelectedImage,
) -> Result<(), ExecutionError> {
    let (profile, profile_name) = profile_name(image.profile)
        .ok_or_else(|| image_error("the selected profile is not an approved workload image"))?;
    let architecture = image
        .platform
        .strip_prefix("linux/")
        .ok_or_else(|| image_error("the selected image has an unsupported platform"))?;
    let snapshot = engine
        .inspect_snapshot()
        .await
        .map_err(|failure| image_error(failure.message))?;
    let locked_image = ImageLock::bundled()
        .and_then(|lock| lock.select(profile, &snapshot.architecture))
        .map_err(|failure| image_error(failure.message))?;
    if &locked_image != image {
        return Err(image_error(
            "the selected workload image does not match the bundled image lock",
        ));
    }
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .ok_or_else(|| image_error("the Docker Engine identity could not be verified"))?;
    let task_id = format!("setup:images:{profile_name}:{architecture}:{engine_identity}");

    let _guard = PREPARATION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let store = crate::persistence::setup_tasks::load()
        .map_err(|_| image_error("machine setup state could not be read"))?;
    if store.has_valid_image_preparation_receipt(&task_id)
        && engine.verify_image_present(image).await.is_ok()
    {
        return Ok(());
    }

    crate::persistence::setup_tasks::require_image_profile(profile)
        .map_err(|_| image_error("the workload image requirement could not be recorded"))?;
    publish_image_task(&task_id)?;

    let operation_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let scopes = vec![task_id.clone()];
    let requested_at = now_ms();
    crate::persistence::setup_tasks::apply_transition(SetupTaskTransitionV1::RequestAttempt {
        task_id: task_id.clone(),
        operation_id: operation_id.clone(),
        version: 1,
        requested_scopes: scopes.clone(),
        at_unix_ms: requested_at,
    })
    .map_err(|_| image_error("the image preparation attempt could not be recorded"))?;

    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host: Host::Linux,
        prerequisite_id: task_id.clone(),
        image_profile: Some(profile_name.to_owned()),
    };
    let (receipt_id, approval) =
        crate::app::bootstrap_approval::test_support::approve_image_for_test(&proposal)
            .await
            .map_err(|error| image_error(format!("test operator approval failed: {error}")))?;
    let authorized_at = now_ms();
    crate::persistence::setup_tasks::apply_transition(SetupTaskTransitionV1::RecordAuthorization {
        task_id: task_id.clone(),
        operation_id: operation_id.clone(),
        binding: AuthorizationBindingV1 {
            receipt_id,
            scope_digest: authorization_scope_digest(&task_id, &operation_id, 1, &scopes),
            authorized_at_unix_ms: authorized_at,
        },
        at_unix_ms: authorized_at,
    })
    .map_err(|_| image_error("the approved image action could not be recorded"))?;

    let (progress, _progress_receiver) = mpsc::sync_channel(128);
    let receipt = engine
        .test_approved_image(
            staging_root,
            approval,
            &crate::execution::docker::engine::ImagePullControl::new(progress),
        )
        .await
        .map_err(|failure| image_error(failure.message))?;
    let completed_at = now_ms();
    crate::persistence::setup_tasks::apply_transition(
        SetupTaskTransitionV1::RecordImagePreparationReceipt {
            task_id: task_id.clone(),
            operation_id: operation_id.clone(),
            receipt: ImagePreparationReceiptV1 {
                profile: receipt.profile,
                architecture: receipt.architecture,
                image_digest: receipt.image_digest,
                engine_identity_fingerprint: receipt.engine_identity_fingerprint,
                successful_smoke_at_unix_ms: completed_at,
            },
            at_unix_ms: completed_at,
        },
    )
    .map_err(|_| image_error("the successful smoke receipt could not be persisted"))?;
    crate::persistence::setup_tasks::apply_transition(SetupTaskTransitionV1::AwaitVerification {
        task_id: task_id.clone(),
        operation_id,
        at_unix_ms: now_ms(),
    })
    .map_err(|_| image_error("the verified image task could not be settled"))?;

    let store = crate::persistence::setup_tasks::load()
        .map_err(|_| image_error("the persisted image receipt could not be read"))?;
    if !store.has_valid_image_preparation_receipt(&task_id) {
        return Err(image_error(
            "the persisted image receipt does not match the current lock and Docker Engine",
        ));
    }
    Ok(())
}

fn publish_image_task(task_id: &str) -> Result<(), ExecutionError> {
    let generation = crate::persistence::setup_tasks::begin_scan()
        .map_err(|_| image_error("a fresh machine setup scan could not start"))?;
    let result =
        crate::app::setup_attention::scan(generation, std::sync::Arc::new(AtomicBool::new(false)));
    let snapshot = result
        .snapshot
        .ok_or_else(|| image_error("a fresh machine setup scan did not complete"))?;
    let store = crate::persistence::setup_tasks::reconcile_live(&snapshot, generation)
        .map_err(|_| image_error("the current machine setup scan could not be saved"))?;
    if store.tasks.contains_key(task_id) {
        Ok(())
    } else {
        Err(image_error(
            "the current machine setup scan did not publish this engine-bound image task",
        ))
    }
}

fn profile_name(profile: RuntimeProfile) -> Option<(RuntimeProfile, &'static str)> {
    Some((
        profile,
        match profile {
            RuntimeProfile::General => "general",
            RuntimeProfile::Rust => "rust",
            RuntimeProfile::Node => "node",
            RuntimeProfile::Python => "python",
            RuntimeProfile::Go => "go",
            RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa => return None,
        },
    ))
}

fn host_platform() -> HostPlatform {
    #[cfg(target_os = "macos")]
    return HostPlatform::MacOs;
    #[cfg(target_os = "windows")]
    return HostPlatform::Windows;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    HostPlatform::Linux
}

fn now_ms() -> u64 {
    crate::persistence::harness_settings::current_unix_ms()
}

fn image_error(message: impl Into<String>) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::ImageUnavailable, message, false)
}
