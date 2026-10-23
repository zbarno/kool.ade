//! Fail-closed policy for app-owned runtime QA driver containers.
//!
//! This policy is intentionally separate from the general executor sandbox:
//! QA needs a read-only build mount and, for browser runs, a private service
//! network. It never accepts scenario-provided Docker fields.

use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, HostConfig};

use crate::execution::{
    contract::ResourceBudgetV1,
    docker::images::{ImageLock, RuntimeProfile, SelectedImage},
    errors::{ExecutionError, ExecutionErrorKind},
};

const OWNER_LABEL: &str = "com.koolade.qa.owner";
const RUN_LABEL: &str = "com.koolade.qa.run";
const TASK_LABEL: &str = "com.koolade.qa.task";
const DRIVER_LABEL: &str = "com.koolade.qa.driver";
const TASK_CONTAINER_LABEL: &str = "com.koolade.qa.task-container";

mod artifact;
mod driver_cache;
mod driver_spec;
mod service;
pub(crate) use service::run_created_driver_with_status;
#[cfg(test)]
#[path = "qa_container/tests.rs"]
mod tests;
pub use artifact::{QaArtifactMount, QaContainerUpload, snapshot_selected_files};
pub(crate) use service::archive_for_runtime_user;
pub use service::{
    DriverContainerOutput, cleanup_driver_container, cleanup_web_preview, create_driver_container,
    recover_driver_container, recover_owned_web_preview, run_created_driver, run_ephemeral_driver,
    start_web_preview, validate_browser_driver_network, validate_preview_relay_network,
    validate_service_driver_network, validate_task_service_network,
};

/// Network is either fully disabled or restricted to a separately inspected
/// internal network that contains the leased task service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QaDriverNetwork {
    Disabled,
    InternalPreviewRelay {
        network_id: String,
        relay_container_id: String,
    },
}

/// App-owned inputs for a single ephemeral QA driver container.
#[derive(Clone)]
pub struct QaDriverContainerSpec<'a> {
    pub owner_id: &'a str,
    pub request: &'a crate::runtime_qa::QaRunRequestV1,
    pub task_uid: &'a str,
    pub driver: &'a str,
    pub image: &'a SelectedImage,
    pub artifacts: &'a [QaArtifactMount],
    pub(crate) dependency_caches: &'a [crate::harness::resource_bridge::CacheSnapshot],
    pub network: QaDriverNetwork,
    pub service_lease: Option<&'a crate::runtime_qa::QaTaskServiceLeaseV1>,
    pub relay_binding: Option<&'a crate::runtime_qa::QaPreviewRelayBindingV1>,
    pub resources: &'a ResourceBudgetV1,
}

/// Inputs for an app-owned Node web preview built from an exact passing
/// receipt and approved start recipe. This never accepts command text.
pub struct QaWebPreviewSpec<'a> {
    pub request: &'a crate::runtime_qa::QaRunRequestV1,
    pub browser_image: &'a SelectedImage,
    pub service_image: &'a SelectedImage,
    pub recipe: &'a crate::project_runtime::ExecutionRecipeV1,
    pub artifact_upload: &'a QaContainerUpload,
    pub(crate) dependency_caches: &'a [crate::harness::resource_bridge::CacheSnapshot],
    pub preview_port: u16,
    pub health_path: &'a str,
    pub resources: &'a ResourceBudgetV1,
}

#[derive(Clone, Debug)]
pub struct QaWebPreview {
    pub service_lease: crate::runtime_qa::QaTaskServiceLeaseV1,
    pub relay_binding: crate::runtime_qa::QaPreviewRelayBindingV1,
}

impl QaDriverContainerSpec<'_> {
    /// Build a fixed, non-root, read-only-rootfs container configuration.
    /// Browser images remain unlaunchable until ImageLock enables their
    /// reviewed digest; this check deliberately occurs at launch time.
    pub fn create_body(&self) -> Result<ContainerCreateBody, ExecutionError> {
        let lock = ImageLock::bundled().map_err(|_| denied("QA image lock is invalid"))?;
        self.create_body_for_lock(&lock)
    }

    #[cfg(test)]
    fn create_body_with_lock(
        &self,
        lock: &ImageLock,
    ) -> Result<ContainerCreateBody, ExecutionError> {
        self.create_body_for_lock(lock)
    }

    fn create_body_for_lock(
        &self,
        lock: &ImageLock,
    ) -> Result<ContainerCreateBody, ExecutionError> {
        self.validate()?;
        let architecture = self
            .image
            .platform
            .strip_prefix("linux/")
            .unwrap_or_default();
        let approved = lock
            .select(self.image.profile, architecture)
            .map_err(|_| denied("QA driver image is not enabled by the reviewed image lock"))?;
        if approved != *self.image
            || !matches!(
                self.image.profile,
                RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa
            )
        {
            return Err(denied("QA driver image is not an approved QA image"));
        }
        let network_mode = match &self.network {
            QaDriverNetwork::Disabled => "none".to_owned(),
            QaDriverNetwork::InternalPreviewRelay { network_id, .. } => network_id.clone(),
        };
        let temporary_bytes = self.resources.temporary_disk_bytes / 4;
        let mut tmpfs = HashMap::from([
            (
                "/tmp".into(),
                format!(
                    "rw,nosuid,nodev,noexec,size={temporary_bytes},uid=10001,gid=10001,mode=1777"
                ),
            ),
            (
                "/run".into(),
                format!(
                    "rw,nosuid,nodev,noexec,size={temporary_bytes},uid=10001,gid=10001,mode=0755"
                ),
            ),
            (
                "/home/executor".into(),
                format!(
                    "rw,nosuid,nodev,noexec,size={temporary_bytes},uid=10001,gid=10001,mode=0755"
                ),
            ),
        ]);
        if self
            .artifacts
            .iter()
            .any(|artifact| artifact.target() == "/workspace")
        {
            tmpfs.insert(
                "/workspace".into(),
                format!("rw,nosuid,nodev,size={temporary_bytes},uid=10001,gid=10001,mode=0755"),
            );
        }
        let mut env = vec!["HOME=/tmp".into(), "CI=true".into(), "LANG=C.UTF-8".into()];
        driver_cache::configure_desktop_cache_mounts(
            self.driver,
            self.dependency_caches,
            self.image,
            self.resources,
            &mut tmpfs,
            &mut env,
        )?;
        Ok(ContainerCreateBody {
            user: Some(self.image.runtime_user.clone()),
            env: Some(env),
            attach_stdin: Some(false),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(false),
            open_stdin: Some(false),
            stdin_once: Some(false),
            image: Some(self.image.reference.clone()),
            working_dir: Some("/workspace".into()),
            labels: Some(HashMap::from([
                (OWNER_LABEL.into(), self.owner_id.into()),
                (RUN_LABEL.into(), self.request.qa_run_id.clone()),
                (TASK_LABEL.into(), self.task_uid.into()),
                (DRIVER_LABEL.into(), self.driver.into()),
                (
                    TASK_CONTAINER_LABEL.into(),
                    match &self.network {
                        QaDriverNetwork::InternalPreviewRelay {
                            relay_container_id, ..
                        } => relay_container_id.clone(),
                        QaDriverNetwork::Disabled => String::new(),
                    },
                ),
            ])),
            host_config: Some(HostConfig {
                privileged: Some(false),
                readonly_rootfs: Some(true),
                network_mode: Some(network_mode),
                ipc_mode: Some("private".into()),
                cap_drop: Some(vec!["ALL".into()]),
                cap_add: Some(Vec::new()),
                security_opt: Some(vec!["no-new-privileges:true".into()]),
                memory: Some(
                    i64::try_from(self.resources.memory_bytes).map_err(|_| {
                        denied("QA memory limit is outside Docker's supported range")
                    })?,
                ),
                memory_swap: Some(
                    i64::try_from(self.resources.memory_bytes).map_err(|_| {
                        denied("QA memory limit is outside Docker's supported range")
                    })?,
                ),
                cpu_period: Some(100_000),
                cpu_quota: Some(i64::from(self.resources.cpu_millis) * 100),
                pids_limit: Some(i64::from(self.resources.process_count)),
                init: Some(true),
                auto_remove: Some(false),
                mounts: Some(Vec::new()),
                binds: Some(Vec::new()),
                tmpfs: Some(tmpfs),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn validate(&self) -> Result<(), ExecutionError> {
        driver_spec::validate(self)
    }
}

pub(super) fn denied(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false)
}

pub(super) fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
