use super::super::*;
#[cfg(target_os = "linux")]
use zeroize::Zeroize;

#[cfg(target_os = "linux")]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedGuestImageLockV1 {
    schema_version: u8,
    status: String,
    manifest_sha256: Option<String>,
    catalog_generation: Option<u64>,
    images: Vec<String>,
}

#[cfg(target_os = "linux")]
fn current_cgroup_v2_path() -> Result<std::path::PathBuf, &'static str> {
    let contents =
        std::fs::read_to_string("/proc/self/cgroup").map_err(|_| "trusted_runtime_unavailable")?;
    cgroup_v2_path_from_proc(&contents, std::path::Path::new("/sys/fs/cgroup"))
}

#[cfg(target_os = "linux")]
fn cgroup_v2_path_from_proc(
    contents: &str,
    mount_root: &std::path::Path,
) -> Result<std::path::PathBuf, &'static str> {
    let mut paths = contents.lines().filter_map(|line| {
        let mut fields = line.splitn(3, ':');
        match (fields.next()?, fields.next()?, fields.next()?) {
            ("0", "", path) => Some(path),
            _ => None,
        }
    });
    let relative = paths.next().ok_or("trusted_runtime_unavailable")?;
    if paths.next().is_some() || !relative.starts_with('/') {
        return Err("trusted_runtime_unavailable");
    }
    let relative = std::path::Path::new(relative);
    if relative.components().any(|component| {
        !matches!(
            component,
            std::path::Component::RootDir | std::path::Component::Normal(_)
        )
    }) {
        return Err("trusted_runtime_unavailable");
    }
    let path = mount_root.join(
        relative
            .strip_prefix("/")
            .map_err(|_| "trusted_runtime_unavailable")?,
    );
    let canonical_mount =
        std::fs::canonicalize(mount_root).map_err(|_| "trusted_runtime_unavailable")?;
    let canonical_path = std::fs::canonicalize(&path).map_err(|_| "trusted_runtime_unavailable")?;
    if canonical_path != path || !canonical_path.starts_with(canonical_mount) {
        return Err("trusted_runtime_unavailable");
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn current_host_machine_id() -> Result<String, &'static str> {
    let value = std::fs::read_to_string("/etc/machine-id")
        .map_err(|_| "trusted_host_identity_unavailable")?;
    let value = value.trim();
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("trusted_host_identity_unavailable");
    }
    let uid = unsafe { libc::geteuid() };
    Ok(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("koolade-computer-use-host-v1:{value}:uid:{uid}").as_bytes(),
    )
    .to_string())
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn cgroup_discovery_accepts_only_one_existing_unified_hierarchy_path() {
    let root = tempfile::tempdir().unwrap();
    let cgroup = root.path().join("user.slice").join("app.slice");
    std::fs::create_dir_all(&cgroup).unwrap();
    let proc = format!(
        "0::/{}\n",
        cgroup.strip_prefix(root.path()).unwrap().display()
    );
    assert_eq!(
        cgroup_v2_path_from_proc(&proc, root.path()).unwrap(),
        cgroup
    );
    assert!(cgroup_v2_path_from_proc("2:cpu:/user.slice\n", root.path()).is_err());
    assert!(cgroup_v2_path_from_proc("0::/../etc\n", root.path()).is_err());
    assert!(cgroup_v2_path_from_proc("0::/user.slice\n0::/app.slice\n", root.path()).is_err());
}

impl KooladeApp {
    pub(super) fn computer_use_run_context(
        &self,
        project_id: uuid::Uuid,
    ) -> Option<crate::core::implementation::ComputerUseRunContext> {
        #[cfg(target_os = "linux")]
        {
            use crate::computer_use::mcp::{
                ComputerUseMcpHostV1, RegistryBoundComputerUseMcpHostV1,
            };
            use std::sync::Arc;

            let operations = self.shared_computer_use_controller();
            let host: Arc<dyn ComputerUseMcpHostV1> =
                Arc::new(RegistryBoundComputerUseMcpHostV1::new(
                    self.computer_use_authority.clone(),
                    self.computer_use_consent.clone(),
                    operations,
                ));
            let uid = unsafe { libc::geteuid() };
            let user_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!("koolade-linux-effective-uid-v1:{uid}").as_bytes(),
            );
            Some(crate::core::implementation::ComputerUseRunContext {
                project_id: project_id.to_string(),
                user_id: user_id.to_string(),
                session_id: uuid::Uuid::new_v4().to_string(),
                host,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = project_id;
            None
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn shared_computer_use_controller(
        &self,
    ) -> std::sync::Arc<crate::computer_use::mcp::ComputerUseMcpControllerV1> {
        let mut current = self
            .computer_use_controller
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current
            .as_ref()
            .is_none_or(|controller| !controller.is_available())
        {
            *current = Some(std::sync::Arc::new(current_host_controller(
                self.computer_use_authority.clone(),
            )));
        }
        current
            .as_ref()
            .expect("controller was initialized")
            .clone()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn revoke_computer_use_session(&self, session_id: &str) -> Result<(), String> {
        let current = self
            .computer_use_controller
            .lock()
            .map_err(|_| "Computer Use controller state is unavailable".to_owned())?;
        if let Some(controller) = current
            .as_ref()
            .filter(|controller| controller.is_available())
        {
            return controller.revoke(session_id).map_err(|reason| {
                format!("Computer Use permission could not be revoked: {reason}")
            });
        }
        self.computer_use_authority
            .revoke(session_id)
            .map_err(|error| format!("Computer Use permission could not be revoked: {error:?}"))
    }
}

#[cfg(target_os = "linux")]
fn current_host_controller(
    registry: std::sync::Arc<crate::computer_use::guest_protocol::ComputerUseAuthorityRegistryV1>,
) -> crate::computer_use::mcp::ComputerUseMcpControllerV1 {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::computer_use::{
        ComputerRemoteWorkerTrustV1, ComputerResourceLimitsV1, ComputerTargetV1,
        LocalKvmComputerBackend,
        artifacts::ComputerArtifactBridgeV1,
        guest_image::{
            ComputerGuestArchitectureV1, GuestImageCacheV1, GuestImageCatalogGenerationFloorV1,
            application_guest_image_manifest_trust_v1, probe_host_qemu_image_tool,
        },
        mcp::{ComputerUseMcpControllerV1, ComputerUseMcpRuntimeConfigV1},
    };

    let result = (|| {
        let storage = crate::persistence::setup_tasks::computer_use_storage_dir()
            .map_err(|_| "trusted_runtime_unavailable")?;
        let transfer_store = PlanningStore::new(
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                b"koolade-computer-use-private-transfers-v1",
            ),
            storage.join("transfers-v1"),
            StoreMode::ManagedLocal,
        );
        let artifact_bridge = ComputerArtifactBridgeV1::new(transfer_store);
        let selected_worker = crate::persistence::computer_use_remote_workers::selected()
            .map_err(|_| "remote_worker_enrollment_unavailable")?;
        let local_machine_id = if selected_worker.is_none() {
            Some(current_host_machine_id()?)
        } else {
            None
        };
        let cache = GuestImageCacheV1::open_existing(storage.join("images"))
            .map_err(|_| "trusted_image_unavailable")?;
        let generation_floor =
            GuestImageCatalogGenerationFloorV1::open(storage.join("catalog-floor"))
                .map_err(|_| "trusted_image_unavailable")?;
        let trust =
            application_guest_image_manifest_trust_v1().map_err(|_| "trusted_image_unavailable")?;
        let qemu_tool = probe_host_qemu_image_tool().map_err(|_| "trusted_runtime_unavailable")?;
        let manifest_bytes =
            include_bytes!("../../../../../containers/computer-use/catalog-v1.json").to_vec();
        let application_catalog_bytes = load_application_catalog(&storage);
        let lock_bytes = include_bytes!("../../../../../containers/computer-use/images.lock.json");
        let lock: EmbeddedGuestImageLockV1 =
            serde_json::from_slice(lock_bytes).map_err(|_| "trusted_image_unavailable")?;
        let cached = cache
            .verify_cached_catalog_image(
                &manifest_bytes,
                &trust,
                &generation_floor,
                ComputerGuestArchitectureV1::X86_64,
            )
            .map_err(|_| "trusted_image_unavailable")?;
        if lock.schema_version != 1
            || lock.status != "promoted"
            || lock.manifest_sha256.as_deref() != Some(cached.manifest_digest())
            || lock.catalog_generation != Some(cached.catalog_generation())
            || !lock
                .images
                .iter()
                .any(|digest| digest == &cached.entry().image_digest)
        {
            return Err("trusted_image_unavailable");
        }
        let (controller_key_id, controller_key) =
            crate::persistence::computer_use_controller_key::load_or_create_computer_use_controller_key_v1()
                .map_err(|_| "trusted_controller_key_unavailable")?;
        let (backend, target, machine_id): (
            std::sync::Arc<dyn crate::computer_use::ComputerBackend>,
            ComputerTargetV1,
            String,
        ) = if let Some(enrollment) = selected_worker {
            enrollment
                .validate()
                .map_err(|_| "remote_worker_enrollment_unavailable")?;
            let target = ComputerTargetV1::RemoteVmWorker {
                worker_id: enrollment.worker_id.clone(),
                machine_id: enrollment.machine_id.clone(),
                trust: ComputerRemoteWorkerTrustV1 {
                    ssh_host_key_fingerprint: enrollment.ssh_host_key_fingerprint.clone(),
                    worker_attestation_key_id: enrollment.worker_attestation_key_id.clone(),
                    worker_attestation_verifying_key_hex: enrollment
                        .worker_attestation_verifying_key_hex
                        .clone(),
                    transport_protocol_version: enrollment.transport_protocol_version,
                    guest_protocol_version: enrollment.guest_protocol_version,
                },
            };
            let backend = crate::computer_use::remote_worker::RemoteComputerBackendV1::new(
                enrollment.clone(),
                std::path::PathBuf::new(),
                std::path::PathBuf::new(),
                controller_key_id.clone(),
                controller_key.verifying_key(),
                storage.join("remote-worker-controller"),
            )
            .map_err(|_| "remote_worker_enrollment_unavailable")?;
            (std::sync::Arc::new(backend), target, enrollment.machine_id)
        } else {
            let machine_id = local_machine_id.expect("local machine identity was loaded");
            let cgroup_root = current_cgroup_v2_path()?;
            let backend = LocalKvmComputerBackend::new(
                storage.join("runtime"),
                storage.join("overlays"),
                cgroup_root,
                machine_id.clone(),
                controller_key_id.clone(),
                controller_key.verifying_key(),
                ComputerResourceLimitsV1::default(),
            )
            .map_err(|_| "trusted_runtime_unavailable")?
            .with_application_catalog_bytes(application_catalog_bytes.clone());
            (
                std::sync::Arc::new(backend),
                ComputerTargetV1::LocalKvm {
                    machine_id: machine_id.clone(),
                },
                machine_id,
            )
        };
        Ok::<_, &'static str>(ComputerUseMcpRuntimeConfigV1 {
            backend,
            target,
            controller_key,
            controller_key_id,
            machine_id,
            manifest_bytes,
            trust,
            cache,
            generation_floor,
            overlay_root: storage.join("overlays"),
            qemu_tool,
            application_catalog_bytes,
            artifact_bridge,
        })
    })();
    match result {
        Ok(config) => ComputerUseMcpControllerV1::new(config, registry.clone())
            .unwrap_or_else(|reason| ComputerUseMcpControllerV1::unavailable(registry, reason)),
        Err(reason) => ComputerUseMcpControllerV1::unavailable(registry, reason),
    }
}

#[cfg(target_os = "linux")]
fn load_application_catalog(storage: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    const MAX_CATALOG_BYTES: u64 = 256 * 1024;
    let path = storage.join("guest-app-catalog-v1.json");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file()
        || metadata.size() == 0
        || metadata.size() > MAX_CATALOG_BYTES
    {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).ok()?;
    let mut bytes = Vec::with_capacity(metadata.size() as usize);
    file.take(MAX_CATALOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_CATALOG_BYTES).then_some(bytes)
}
