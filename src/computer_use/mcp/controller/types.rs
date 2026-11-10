mod display;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use super::super::{ComputerUseActionSummaryV1, ComputerUseControlOwnerV1};
use super::scheduler::{ComputerUseSchedulerV1, SessionPermit};
use super::session_journal::ComputerUseSessionJournalV1;
use crate::computer_use::artifacts::ComputerArtifactBridgeV1;
use crate::computer_use::{
    ComputerActionApprovalV1, ComputerActionIntentStateV1, ComputerActionIntentV1,
    ComputerActionPayloadV1, ComputerBackend, ComputerMachineHandleV1, ComputerSessionV1,
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
    pub target: crate::computer_use::ComputerTargetV1,
    pub controller_key: ed25519_dalek::SigningKey,
    pub controller_key_id: String,
    pub machine_id: String,
    pub manifest_bytes: Vec<u8>,
    pub trust: GuestImageManifestTrustV1,
    pub cache: GuestImageCacheV1,
    pub generation_floor: GuestImageCatalogGenerationFloorV1,
    pub overlay_root: PathBuf,
    pub qemu_tool: QemuImageToolV1,
    /// Optional signed application catalog. Missing or invalid data disables
    /// package operations without taking down ordinary guest sessions.
    pub application_catalog_bytes: Option<Vec<u8>>,
    /// App-owned P15 bridge shared with staged task application launch.
    pub artifact_bridge: ComputerArtifactBridgeV1,
}

pub(super) struct Runtime {
    pub backend: Arc<dyn ComputerBackend>,
    pub target: crate::computer_use::ComputerTargetV1,
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
    pub scheduler: Arc<ComputerUseSchedulerV1>,
    pub session_journal: ComputerUseSessionJournalV1,
    pub personal_profiles: Option<crate::computer_use::PersonalVmProfileStoreV1>,
    pub application_catalog: Option<crate::computer_use::apps::VerifiedGuestApplicationCatalogV1>,
    pub application_catalog_status: &'static str,
    pub artifact_bridge: ComputerArtifactBridgeV1,
}

pub(super) struct ActiveSession {
    pub session: ComputerSessionV1,
    pub handle: ComputerMachineHandleV1,
    pub journal: GuestActionJournalV1,
    pub channel: Option<AuthenticatedGuestChannelV1>,
    pub started: bool,
    pub paused: bool,
    pub uncertain: bool,
    pub uncertain_action_sequence: Option<u64>,
    pub inspection_result: Option<String>,
    pub next_action_sequence: u64,
    pub role_id: String,
    pub control_owner: ComputerUseControlOwnerV1,
    pub control_generation: u64,
    pub require_fresh_agent_observation: bool,
    pub private_input: bool,
    pub active_app: Option<String>,
    pub personal_profile_id: Option<String>,
    pub action_log: std::collections::VecDeque<ComputerUseActionSummaryV1>,
    pub pending_action_intent: Option<PendingComputerActionIntentV1>,
    pub display_io: Arc<Mutex<()>>,
    pub _scheduler_permit: SessionPermit,
}

pub(super) struct PendingComputerActionIntentV1 {
    pub intent: ComputerActionIntentV1,
    pub final_action: ComputerActionPayloadV1,
    pub state: ComputerActionIntentStateV1,
    pub approval: Option<ComputerActionApprovalV1>,
}

pub(super) struct PendingPersonalVmConsentV1 {
    pub owner_id: String,
    pub view: crate::computer_use::mcp::ComputerUsePersonalVmConsentViewV1,
}

pub(super) struct DisplaySnapshot {
    pub(super) last_requested: Instant,
    pub(super) frame_revision: u64,
    pub(super) frame_width: u16,
    pub(super) frame_height: u16,
    pub(super) frame_png: Option<Vec<u8>>,
    pub(super) frame_error: bool,
}

pub struct ComputerUseMcpControllerV1 {
    pub(super) registry: Arc<ComputerUseAuthorityRegistryV1>,
    pub(super) runtime: Option<Runtime>,
    pub(super) unavailable_reason: Option<String>,
    pub(super) sessions: Mutex<BTreeMap<String, ActiveSession>>,
    pub(super) display_streams: Mutex<BTreeMap<String, Arc<Mutex<DisplaySnapshot>>>>,
    pub(super) view_cache: Mutex<BTreeMap<String, super::super::ComputerUseTaskSessionViewV1>>,
    pub(super) pending_controls: Mutex<BTreeMap<String, Option<String>>>,
    pub(super) pending_personal_vm_consents: Mutex<BTreeMap<String, PendingPersonalVmConsentV1>>,
    pub(super) recovery_notices: Mutex<BTreeMap<String, String>>,
}

impl ComputerUseMcpControllerV1 {
    pub fn is_available(&self) -> bool {
        self.runtime.is_some()
    }

    /// Return the controller's shared P15 bridge to trusted app code. This
    /// carries no session key or action authority.
    pub fn artifact_bridge(&self) -> Option<ComputerArtifactBridgeV1> {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.artifact_bridge.clone())
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
            display_streams: Mutex::new(BTreeMap::new()),
            view_cache: Mutex::new(BTreeMap::new()),
            pending_controls: Mutex::new(BTreeMap::new()),
            pending_personal_vm_consents: Mutex::new(BTreeMap::new()),
            recovery_notices: Mutex::new(BTreeMap::new()),
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
        if config.target.validate().is_err()
            || config.target.target_machine_id() != config.machine_id
        {
            return Err("trusted_target_identity_unavailable");
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
        let (application_catalog, application_catalog_status) =
            verify_application_catalog(config.application_catalog_bytes.as_deref(), &image);
        let scheduler = ComputerUseSchedulerV1::new(
            probe.parallel_slots_per_machine,
            probe.max_vcpus,
            probe.max_memory_bytes,
            probe.max_overlay_bytes,
        )
        .ok_or("trusted_scheduler_unavailable")?;
        let session_journal = ComputerUseSessionJournalV1::open(
            &config.overlay_root.with_file_name("session-journal"),
        )?;
        let personal_profiles = crate::computer_use::PersonalVmProfileStoreV1::open(
            config.overlay_root.join("personal"),
            "/usr/bin/secret-tool",
        )
        .ok();
        config
            .backend
            .recover()
            .map_err(|_| "trusted_runtime_recovery_failed")?;
        session_journal.reconcile_after_restart(super::session::unix_time_ms())?;
        Ok(Self {
            registry,
            runtime: Some(Runtime {
                backend: config.backend,
                target: config.target,
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
                scheduler,
                session_journal,
                personal_profiles,
                application_catalog,
                application_catalog_status,
                artifact_bridge: config.artifact_bridge,
            }),
            unavailable_reason: None,
            sessions: Mutex::new(BTreeMap::new()),
            display_streams: Mutex::new(BTreeMap::new()),
            view_cache: Mutex::new(BTreeMap::new()),
            pending_controls: Mutex::new(BTreeMap::new()),
            pending_personal_vm_consents: Mutex::new(BTreeMap::new()),
            recovery_notices: Mutex::new(BTreeMap::new()),
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

fn verify_application_catalog(
    bytes: Option<&[u8]>,
    image: &CachedGuestImageV1,
) -> (
    Option<crate::computer_use::apps::VerifiedGuestApplicationCatalogV1>,
    &'static str,
) {
    use crate::computer_use::apps::{
        GuestAppArchitectureV1, GuestApplicationCatalogTrustV1, GuestApplicationCatalogV1,
        VerifiedGuestApplicationCatalogV1,
    };

    let Some(bytes) = bytes else {
        return (None, "signed_app_catalog_unavailable");
    };
    if bytes.is_empty() || bytes.len() > 256 * 1024 {
        return (None, "signed_app_catalog_invalid");
    }
    let Ok(catalog) = serde_json::from_slice::<GuestApplicationCatalogV1>(bytes) else {
        return (None, "signed_app_catalog_invalid");
    };
    let Ok(trust) = GuestApplicationCatalogTrustV1::application_pinned() else {
        return (None, "signed_app_catalog_trust_unavailable");
    };
    let Ok(verified) = VerifiedGuestApplicationCatalogV1::verify(catalog, &trust) else {
        return (None, "signed_app_catalog_invalid");
    };
    if verified.catalog().image_digest != image.entry().image_digest
        || verified.catalog().architecture != GuestAppArchitectureV1::X86_64
    {
        return (None, "signed_app_catalog_image_mismatch");
    }
    (Some(verified), "signed_app_catalog_ready")
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
