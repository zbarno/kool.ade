use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::computer_use::{
    ComputerBackend, ComputerMachineHandleV1, ComputerSessionV1,
    guest_image::{
        CachedGuestImageV1, GuestImageCacheV1, GuestImageCatalogGenerationFloorV1,
        GuestImageManifestTrustV1, QemuImageToolV1,
    },
    guest_protocol::{
        AuthenticatedGuestChannelV1, ComputerUseAuthorityRegistryV1, GuestActionJournalV1,
    },
};

pub struct ComputerUseMcpRuntimeConfigV1 {
    pub backend: Arc<dyn ComputerBackend>,
    pub controller_key: ed25519_dalek::SigningKey,
    pub controller_key_id: String,
    pub machine_id: String,
    pub manifest_bytes: Vec<u8>,
    pub trust: GuestImageManifestTrustV1,
    pub cache: GuestImageCacheV1,
    pub generation_floor: GuestImageCatalogGenerationFloorV1,
    pub overlay_root: PathBuf,
    pub qemu_tool: QemuImageToolV1,
}

pub(super) struct Runtime {
    pub backend: Arc<dyn ComputerBackend>,
    pub controller_key: ed25519_dalek::SigningKey,
    pub controller_key_id: String,
    pub machine_id: String,
    pub manifest_bytes: Vec<u8>,
    pub trust: GuestImageManifestTrustV1,
    pub cache: GuestImageCacheV1,
    pub generation_floor: GuestImageCatalogGenerationFloorV1,
    pub overlay_root: PathBuf,
    pub qemu_tool: QemuImageToolV1,
    pub image: CachedGuestImageV1,
}

pub(super) struct ActiveSession {
    pub session: ComputerSessionV1,
    pub handle: ComputerMachineHandleV1,
    pub journal: GuestActionJournalV1,
    pub channel: Option<AuthenticatedGuestChannelV1>,
    pub started: bool,
    pub paused: bool,
    pub uncertain: bool,
    pub next_action_sequence: u64,
}

pub struct ComputerUseMcpControllerV1 {
    pub(super) registry: Arc<ComputerUseAuthorityRegistryV1>,
    pub(super) runtime: Option<Runtime>,
    pub(super) unavailable_reason: Option<String>,
    pub(super) sessions: Mutex<BTreeMap<String, ActiveSession>>,
}

impl ComputerUseMcpControllerV1 {
    pub fn is_available(&self) -> bool {
        self.runtime.is_some()
    }

    pub fn unavailable(
        registry: Arc<ComputerUseAuthorityRegistryV1>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            registry,
            runtime: None,
            unavailable_reason: Some(reason.into()),
            sessions: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn for_current_host(registry: Arc<ComputerUseAuthorityRegistryV1>) -> Self {
        Self::unavailable(registry, "trusted_host_configuration_unavailable")
    }

    pub fn new(
        config: ComputerUseMcpRuntimeConfigV1,
        registry: Arc<ComputerUseAuthorityRegistryV1>,
    ) -> Result<Self, &'static str> {
        let is_uuid =
            |value: &str| uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value);
        if !is_uuid(&config.machine_id) || !is_uuid(&config.controller_key_id) {
            return Err("trusted_host_identity_unavailable");
        }
        let probe = config
            .backend
            .probe()
            .map_err(|_| "trusted_runtime_unavailable")?;
        if !probe.linux_host
            || !probe.kvm_accessible
            || !probe.qemu_available
            || !probe.delegated_resource_controls
        {
            return Err("trusted_runtime_unavailable");
        }
        let image = config
            .cache
            .verify_cached_catalog_image(
                &config.manifest_bytes,
                &config.trust,
                &config.generation_floor,
                crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
            )
            .map_err(|_| "trusted_image_unavailable")?;
        Ok(Self {
            registry,
            runtime: Some(Runtime {
                backend: config.backend,
                controller_key: config.controller_key,
                controller_key_id: config.controller_key_id,
                machine_id: config.machine_id,
                manifest_bytes: config.manifest_bytes,
                trust: config.trust,
                cache: config.cache,
                generation_floor: config.generation_floor,
                overlay_root: config.overlay_root,
                qemu_tool: config.qemu_tool,
                image,
            }),
            unavailable_reason: None,
            sessions: Mutex::new(BTreeMap::new()),
        })
    }

    pub(super) fn blocked(&self, reason: &str) -> super::super::ComputerUseMcpResponseV1 {
        super::super::ComputerUseMcpResponseV1 {
            schema_version: super::super::COMPUTER_USE_MCP_VERSION_V1,
            status: super::super::ComputerUseMcpStatusV1::Blocked,
            summary: "Computer Use requires trusted host setup before access can continue.".into(),
            reason_code: Some(reason.into()),
            session_id: None,
            capabilities: Vec::new(),
            observation: None,
            action_outcome: None,
        }
    }
}

impl Drop for ComputerUseMcpControllerV1 {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        let Ok(sessions) = self.sessions.get_mut() else {
            return;
        };
        for (session_id, active) in std::mem::take(sessions) {
            let _ = self.registry.revoke(&session_id);
            runtime
                .backend
                .cancel(
                    &active.session,
                    &active.handle,
                    super::session::unix_time_ms(),
                )
                .ok();
            runtime
                .backend
                .release(
                    &active.session,
                    &active.handle,
                    super::session::unix_time_ms(),
                )
                .ok();
        }
    }
}
