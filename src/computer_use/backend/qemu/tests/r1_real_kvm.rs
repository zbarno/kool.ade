use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerBackend,
    ComputerKeyV1, ComputerLifecycleV1, ComputerMachineStateV1, ComputerNetworkProfileV1,
    ComputerOperationV1, KeyCodeV1, MINIMUM_DISPOSABLE_VM_BUDGET_V1, MouseButtonV1,
    guest_image::{
        ComputerGuestArchitectureV1, ComputerImageDownloadApprovalV1, GuestImageCacheV1,
        GuestImageCatalogGenerationFloorV1, GuestImageOverlayRequestV1, ImageCacheErrorV1,
        application_guest_image_manifest_trust_v1, create_guest_image_overlay,
        probe_host_qemu_image_tool,
    },
    guest_protocol::ComputerUseAuthorityRegistryV1,
};

use super::super::{LocalKvmComputerBackend, policy::target_machine_id};
use crate::computer_use::ComputerResourceLimitsV1;

#[path = "r1_real_kvm/cgroup.rs"]
mod cgroup;
#[path = "r1_real_kvm/cleanup.rs"]
mod cleanup;
#[path = "r1_real_kvm/controller_restart.rs"]
mod controller_restart;
#[path = "r1_real_kvm/host_environment.rs"]
mod host_environment;
#[path = "r1_real_kvm/performance.rs"]
mod performance;
#[path = "r1_real_kvm/qa_driver.rs"]
mod qa_driver;
#[path = "r1_real_kvm/qmp.rs"]
mod qmp;
use cgroup::current_cgroup_root;

const REAL_KVM_ENV: &str = "KOOLADE_COMPUTER_USE_R1_REAL_KVM";
const CONTROLLER_RESTART_ROOT_ENV: &str = "KOOLADE_R1_KVM_CONTROLLER_RESTART_ROOT";
const CONTROLLER_RESTART_CHILD_ENV: &str = "KOOLADE_R1_KVM_CONTROLLER_RESTART_CHILD";
const MANIFEST: &[u8] = include_bytes!("../../../../../containers/computer-use/catalog-v1.json");

struct TestImageApproval;

impl ComputerImageDownloadApprovalV1 for TestImageApproval {
    fn authorize_download(
        &self,
        _manifest_digest: &str,
        _image_digest: &str,
    ) -> Result<(), ImageCacheErrorV1> {
        Ok(())
    }
}

/// Production boundary: signed image cache -> QEMU/KVM backend -> authenticated
/// guest companion -> controller-scoped GUI action -> guest and display frames.
/// Component mocks cannot prove that QEMU boots this image, establishes the
/// authenticated channel, launches a real window, or streams its display.
/// Dependencies: a trusted, unprivileged Linux x86_64 KVM host, root-owned
/// QEMU 8.x binaries, delegated cgroup v2, and a public 682 MB image download.
/// Expected setup cost: image download once per fresh temporary test root and
/// up to 120 seconds for guest boot, action and cleanup. QMP inspects the live
/// PCI graph to verify that the disabled-network guest has no network device;
/// a command-builder unit test cannot detect QEMU-added runtime devices. A
/// synthetic runner environment canary also verifies that QEMU inherits no host
/// secrets from its parent process. The test reads the live cgroup controls and
/// process count because QEMU command arguments alone do not prove the kernel
/// enforced the session's CPU, memory, swap, and task limits. An approved
/// native QA fixture crosses the P15 bridge and authenticated guest channel,
/// launches through the guest helper, and runs through the production QA
/// scenario driver.
#[test]
fn computer_use_r1_real_kvm_acceptance() {
    if std::env::var(REAL_KVM_ENV).as_deref() != Ok("1") {
        return;
    }
    if std::env::var(CONTROLLER_RESTART_CHILD_ENV).as_deref() == Ok("1") {
        let root = std::env::var_os(CONTROLLER_RESTART_ROOT_ENV)
            .map(std::path::PathBuf::from)
            .expect("controller-restart child needs a shared runtime root");
        run_real_kvm_acceptance(&root, true)
            .unwrap_or_else(|reason| panic!("R1 real KVM child acceptance failed: {reason}"));
    } else {
        controller_restart::run()
            .unwrap_or_else(|reason| panic!("R1 real KVM controller restart failed: {reason}"));
        let temporary = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("R1 QEMU crash-test root failed: {error}"));
        run_real_kvm_acceptance(temporary.path(), false)
            .unwrap_or_else(|reason| panic!("R1 real QEMU crash acceptance failed: {reason}"));
    }
}

fn run_real_kvm_acceptance(root: &Path, wait_for_controller_restart: bool) -> Result<(), String> {
    // Reuse the public signed image across opt-in retries; failed guest boots
    // should not require another 682 MB download.
    let image_cache_root = std::env::temp_dir().join(format!("koolade-r1-kvm-cache-{}", unsafe {
        libc::geteuid()
    }));
    let cache = GuestImageCacheV1::new(image_cache_root.join("images"))
        .map_err(|error| format!("image cache: {error}"))?;
    let floor_root = image_cache_root.join("catalog-floor");
    let floor = GuestImageCatalogGenerationFloorV1::open(floor_root.clone())
        .or_else(|_| {
            GuestImageCatalogGenerationFloorV1::initialize_for_first_install(floor_root, 1)
        })
        .map_err(|error| format!("catalog floor: {error}"))?;
    let trust = application_guest_image_manifest_trust_v1()
        .map_err(|error| format!("pinned image trust: {error}"))?;
    let image_download_started = Instant::now();
    let image = cache
        .download_approved_catalog_release(
            MANIFEST,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            &TestImageApproval,
        )
        .map_err(|error| format!("approved image download and verification: {error}"))?;
    let image_download_ms = image_download_started.elapsed().as_millis();

    // Start the short-lived session budget only after the potentially slow
    // image download, otherwise the guest test expires before overlay setup.
    let now = unix_time_ms()?;
    let mut session = super::session();
    session.identity.session_id = uuid::Uuid::new_v4().to_string();
    session.policy.identity = session.identity.clone();
    session.session_started_at_unix_ms = now;
    session.image_digest = image.entry().image_digest.clone();
    session.image_manifest_digest = image.manifest_digest().to_owned();
    session.image_signer_key_id = image.manifest_signer_key_id().to_owned();
    session.network_profile = ComputerNetworkProfileV1::Disabled;
    session.allowed_apps = BTreeSet::from([
        "chromium".to_owned(),
        crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.to_owned(),
    ]);
    session.policy.allowed_apps = session.allowed_apps.clone();
    session.policy.authorized_operations = BTreeSet::from([
        ComputerOperationV1::ObserveScreen,
        ComputerOperationV1::MouseInput,
        ComputerOperationV1::KeyboardInput,
        ComputerOperationV1::ReadAccessibility,
        ComputerOperationV1::LaunchAllowedApp,
        ComputerOperationV1::LaunchApprovedStagedTaskApp,
    ]);
    session.policy.resource_budget = MINIMUM_DISPOSABLE_VM_BUDGET_V1;
    // QEMU and its desktop guest share the cgroup memory limit; leave room for
    // QEMU's own device and thread allocations alongside the guest's RAM.
    session.policy.resource_budget.memory_bytes = 2 * 1024 * 1024 * 1024;
    // The approved guest image itself has a 4 GiB virtual disk, so the test
    // session must authorize at least that much overlay capacity.
    session.policy.resource_budget.disk_bytes = image.entry().image_size_bytes;
    session.lifecycle = ComputerLifecycleV1::Preparing;
    session.lease.issued_at_unix_ms = now;
    session.lease.expires_at_unix_ms = now.saturating_add(10 * 60 * 1_000);
    session.policy.network_profile = ComputerNetworkProfileV1::Disabled;
    let controller_key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    let controller_key_id = "00000000-0000-4000-8000-000000000008";
    session
        .attest(controller_key_id.to_owned(), &controller_key)
        .map_err(str::to_owned)?;
    let machine_id = target_machine_id(&session, None)
        .map_err(|error| format!("target: {error:?}"))?
        .to_owned();
    let cgroup_root = current_cgroup_root()?;
    let backend = LocalKvmComputerBackend::new(
        root.join("runtime"),
        root.join("overlays"),
        cgroup_root.clone(),
        machine_id.clone(),
        controller_key_id.to_owned(),
        controller_key.verifying_key(),
        ComputerResourceLimitsV1::default(),
    )
    .map_err(|error| format!("backend configuration: {error:?}"))?;
    let probe = backend
        .probe()
        .map_err(|error| format!("host probe: {error:?}"))?;
    if !probe.linux_host
        || !probe.kvm_accessible
        || !probe.qemu_available
        || !probe.delegated_resource_controls
    {
        return Err("backend reported an incomplete host probe".into());
    }

    let overlay_root = root.join("overlays");
    fs::create_dir(&overlay_root).map_err(|error| error.to_string())?;
    let qemu_tool =
        probe_host_qemu_image_tool().map_err(|error| format!("qemu-img probe: {error:?}"))?;
    let provisioning_started = Instant::now();
    let overlay = create_guest_image_overlay(
        &cache,
        GuestImageOverlayRequestV1 {
            manifest_bytes: MANIFEST,
            trust: &trust,
            session: &session,
            expected_architecture: ComputerGuestArchitectureV1::X86_64,
            controller_key_id,
            controller_key: &controller_key.verifying_key(),
            generation_floor: &floor,
            overlay_root: &overlay_root,
            qemu_tool: &qemu_tool,
        },
    )
    .map_err(|error| format!("signed overlay creation: {error}"))?;
    let overlay_path = overlay.path().to_path_buf();
    let base_image_path = image.path().to_path_buf();

    let (handle, _) = backend
        .prepare(&session, overlay, unix_time_ms()?)
        .map_err(|error| format!("backend prepare: {error:?}"))?;
    let session_directory = root
        .join("runtime/sessions")
        .join(&session.identity.session_id);
    let session_cgroup = cgroup_root.join(format!("koolade-{}", session.identity.session_id));
    let mut provisioning_ms = 0;
    let mut screen_observation_ms = 0;
    let mut idle_sample_window_ms = 0;
    let mut idle_memory_bytes = 0;
    let mut idle_cpu_usec = 0;
    let mut peak_memory_bytes = 0;
    let mut crash_recovery_ms = None;
    let result = (|| {
        backend
            .boot(&session, &handle, unix_time_ms()?)
            .map_err(|error| format!("QEMU/KVM boot: {error:?}"))?;
        cgroup::assert_session_limits(
            &session_cgroup,
            session.policy.resource_budget.cpu_millis,
            session.policy.resource_budget.memory_bytes,
            ComputerResourceLimitsV1::default().max_processes,
        )
        .map_err(|error| format!("live QEMU resource ceilings: {error}"))?;
        cgroup::assert_live_process_count(&session_cgroup)
            .map_err(|error| format!("live QEMU cgroup membership: {error}"))?;
        let monitor_socket = root
            .join("runtime/sessions")
            .join(&session.identity.session_id)
            .join("qemu-monitor.sock");
        let authenticated = backend.authenticate_guest(&session, &handle, unix_time_ms()?);
        let mut channel = authenticated
            .map_err(|error| {
                let log = root
                    .join("runtime/sessions")
                    .join(&session.identity.session_id)
                    .join("qemu-stderr.log");
                let stderr = fs::read(log).unwrap_or_default();
                let stderr = String::from_utf8_lossy(&stderr)
                    .chars()
                    .take(2_000)
                    .collect::<String>();
                let serial = fs::read_to_string(root
                    .join("runtime/sessions")
                    .join(&session.identity.session_id)
                    .join("guest-serial.log"))
                    .unwrap_or_default();
                let serial = serial.chars().take(4_000).collect::<String>();
                let cgroup = cgroup_root.join(format!("koolade-{}", session.identity.session_id));
                let pids_current = fs::read_to_string(cgroup.join("pids.current"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let pids_events = fs::read_to_string(cgroup.join("pids.events"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let memory_current = fs::read_to_string(cgroup.join("memory.current"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let memory_events = fs::read_to_string(cgroup.join("memory.events"))
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                let monitor = monitor_snapshot(&monitor_socket)
                    .unwrap_or_else(|error| format!("unavailable: {error}"));
                format!("guest companion handshake: {error:?}; QEMU stderr: {stderr}; guest serial: {serial}; monitor: {monitor}; cgroup pids.current={pids_current:?}; pids.events={pids_events:?}; memory.current={memory_current:?}; memory.events={memory_events:?}")
            })?;
        backend
            .wait_ready(&session, &handle, Duration::from_secs(120), unix_time_ms()?)
            .map_err(|error| format!("guest readiness: {error:?}"))?;
        provisioning_ms = provisioning_started.elapsed().as_millis();
        qmp::assert_no_network_device(&session_directory.join("qemu-qmp.sock"))
            .map_err(|error| format!("disabled-network QEMU device graph: {error}"))?;
        qmp::assert_approved_pci_devices(&session_directory.join("qemu-qmp.sock"))
            .map_err(|error| format!("QEMU PCI device isolation: {error}"))?;
        qmp::assert_unapproved_network_hotplug_is_denied(&session_directory.join("qemu-qmp.sock"))
            .map_err(|error| format!("live QEMU network hotplug denial: {error}"))?;
        qmp::assert_only_session_chardevs(
            &session_directory.join("qemu-qmp.sock"),
            &session_directory,
        )
        .map_err(|error| format!("QEMU character-device isolation: {error}"))?;
        qmp::assert_only_approved_block_files(
            &session_directory.join("qemu-qmp.sock"),
            &overlay_path,
            &base_image_path,
        )
        .map_err(|error| format!("QEMU host-file isolation: {error}"))?;
        host_environment::assert_host_secret_not_inherited(&session_directory)
            .map_err(|error| format!("host secret isolation: {error}"))?;

        session.lifecycle = ComputerLifecycleV1::Delegated;
        session
            .attest(controller_key_id.to_owned(), &controller_key)
            .map_err(str::to_owned)?;
        let journal = backend
            .open_guest_action_journal(&session, &handle, unix_time_ms()?)
            .map_err(|error| format!("guest action journal: {error:?}"))?;
        let registry = ComputerUseAuthorityRegistryV1::default();
        registry
            .register(
                session.clone(),
                controller_key_id.to_owned(),
                controller_key.verifying_key(),
                ComputerActionActorV1::Agent,
                "implementer/v1".into(),
                unix_time_ms()?,
            )
            .map_err(|error| format!("controller authority registration: {error:?}"))?;
        let authority = registry
            .authorize(
                &session.identity,
                ComputerActionActorV1::Agent,
                "implementer/v1",
                unix_time_ms()?,
            )
            .map_err(|error| format!("controller authority lookup: {error:?}"))?;

        let launch = action(
            &session,
            1,
            ComputerOperationV1::LaunchAllowedApp,
            ComputerActionPayloadV1::LaunchApp {
                app_id: "chromium".into(),
            },
        );
        launch
            .validate_as(ComputerActionActorV1::Agent, &session, unix_time_ms()?)
            .map_err(|reason| format!("local launch action validation: {reason}"))?;
        let launch_result = authority
            .execute(
                &mut channel,
                &journal,
                &launch,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("Chromium launch action: {error:?}"))?;
        let launch_frame = launch_result.observation.ok_or_else(|| {
            let session_root = root
                .join("runtime/sessions")
                .join(&session.identity.session_id);
            let serial = fs::read_to_string(session_root.join("guest-serial.log"))
                .unwrap_or_default();
            let serial = serial.chars().take(4_000).collect::<String>();
            let qemu_stderr = fs::read_to_string(session_root.join("qemu-stderr.log"))
                .unwrap_or_default();
            let qemu_stderr = qemu_stderr.chars().take(2_000).collect::<String>();
            format!(
                "Chromium launch returned no guest frame (result={:?}, guest_capabilities={:?}); guest serial: {serial}; QEMU stderr: {qemu_stderr}",
                launch_result.reply.result,
                channel.capabilities()
            )
        })?;
        if launch_frame.width_px == 0
            || launch_frame.height_px == 0
            || launch_frame.screenshot_png_base64.is_empty()
        {
            return Err("Chromium launch returned an empty guest frame".into());
        }

        let idle_cpu_before = performance::read_cpu_usage_usec(&session_cgroup)?;
        let idle_sample_started = Instant::now();
        thread::sleep(Duration::from_secs(1));
        idle_sample_window_ms = idle_sample_started.elapsed().as_millis();
        idle_memory_bytes = performance::read_cgroup_value(&session_cgroup, "memory.current")?;
        idle_cpu_usec =
            performance::read_cpu_usage_usec(&session_cgroup)?.saturating_sub(idle_cpu_before);

        // Capture the interaction target after the idle measurement. An old
        // frame must not be reused for user input after a slow measurement.
        let observe = action(
            &session,
            2,
            ComputerOperationV1::ObserveScreen,
            ComputerActionPayloadV1::ObserveScreen,
        );
        let screen_observation_started = Instant::now();
        let observation = authority
            .execute(
                &mut channel,
                &journal,
                &observe,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("guest screenshot action: {error:?}"))?
            .observation
            .ok_or("screenshot action returned no guest frame")?;
        screen_observation_ms = screen_observation_started.elapsed().as_millis();
        if observation.session_id != session.identity.session_id
            || observation.observation_revision <= launch_frame.observation_revision
            || observation.width_px == 0
            || observation.height_px == 0
            || observation.screenshot_png_base64.is_empty()
        {
            return Err("authenticated guest screenshot is invalid or stale".into());
        }

        let window = observation
            .windows
            .iter()
            .find(|window| window.app_id.as_deref() == Some("chromium"))
            .ok_or("Chromium window missing from authenticated observation")?;
        let window_id = window.window_id.clone();
        let frame_reference = crate::computer_use::ComputerFrameReferenceV1 {
            frame_id: observation.frame_id.clone(),
            display_generation: observation.display_generation,
            observation_revision: observation.observation_revision,
            display_width_px: observation.width_px,
            display_height_px: observation.height_px,
            device_scale_numerator: observation.device_scale_numerator,
            device_scale_denominator: observation.device_scale_denominator,
            window_id: window.window_id.clone(),
            window_generation: window.generation,
            window_bounds: window.bounds.clone(),
        };
        let click = action(
            &session,
            3,
            ComputerOperationV1::MouseInput,
            ComputerActionPayloadV1::MouseClick {
                x: (window.bounds.left + (window.bounds.width_px / 2) as i32).max(0) as u32,
                y: (window.bounds.top + (window.bounds.height_px / 2) as i32).max(0) as u32,
                target: frame_reference,
                button: MouseButtonV1::Primary,
            },
        );
        let click_result = authority
            .execute(
                &mut channel,
                &journal,
                &click,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest mouse click: {error:?}"))?;
        require_guest_action_completed("real guest mouse click", &click_result.reply.result)?;
        let mut input_observation = observe_frame(&authority, &mut channel, &journal, &session, 4)?;
        let focus_address = action(
            &session,
            5,
            ComputerOperationV1::KeyboardInput,
            ComputerActionPayloadV1::KeyChord {
                keys: vec![
                    ComputerKeyV1::Named(KeyCodeV1::Control),
                    ComputerKeyV1::Character('l'),
                ],
                window_id: window_id.clone(),
                observation_revision: input_observation.observation_revision,
            },
        );
        let focus_result = authority
            .execute(
                &mut channel,
                &journal,
                &focus_address,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest keyboard shortcut: {error:?}"))?;
        require_guest_action_completed("real guest keyboard shortcut", &focus_result.reply.result)?;
        input_observation = observe_frame(&authority, &mut channel, &journal, &session, 6)?;
        let address = action(
            &session,
            7,
            ComputerOperationV1::KeyboardInput,
            ComputerActionPayloadV1::TypeText {
                text: "data:text/html,Koolade%20KVM%20input%20verified".into(),
                window_id: window_id.clone(),
                observation_revision: input_observation.observation_revision,
            },
        );
        let type_result = authority
            .execute(
                &mut channel,
                &journal,
                &address,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest text input: {error:?}"))?;
        require_guest_action_completed("real guest text input", &type_result.reply.result)?;
        input_observation = observe_frame(&authority, &mut channel, &journal, &session, 8)?;
        let enter = action(
            &session,
            9,
            ComputerOperationV1::KeyboardInput,
            ComputerActionPayloadV1::KeyPress {
                key: ComputerKeyV1::Named(KeyCodeV1::Enter),
                window_id: window_id.clone(),
                observation_revision: input_observation.observation_revision,
            },
        );
        let enter_result = authority
            .execute(
                &mut channel,
                &journal,
                &enter,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest Enter key: {error:?}"))?;
        require_guest_action_completed("real guest Enter key", &enter_result.reply.result)?;
        let mut input_frame = observe_frame(&authority, &mut channel, &journal, &session, 10)?;
        let mut next_action_sequence = 11;
        for sequence in 11..=15 {
            if input_frame.screenshot_sha256 != observation.screenshot_sha256 {
                break;
            }
            thread::sleep(Duration::from_millis(200));
            input_frame = observe_frame(&authority, &mut channel, &journal, &session, sequence)?;
            next_action_sequence = sequence + 1;
        }
        if input_frame.screenshot_sha256 == observation.screenshot_sha256 {
            return Err(
                "real guest mouse and keyboard input did not change the screen within one second"
                    .into(),
            );
        }

        // Human takeover atomically revokes the old agent capability. Handback
        // repeats that fence before the agent can act again.
        session.lifecycle = ComputerLifecycleV1::HumanControl;
        session
            .attest(controller_key_id.to_owned(), &controller_key)
            .map_err(str::to_owned)?;
        registry
            .replace_actor(
                session.clone(),
                controller_key_id.to_owned(),
                controller_key.verifying_key(),
                ComputerActionActorV1::Human,
                "implementer/v1".into(),
                unix_time_ms()?,
            )
            .map_err(|error| format!("human takeover authority replacement: {error:?}"))?;
        if authority
            .execute(
                &mut channel,
                &journal,
                &observe,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .is_ok()
        {
            return Err("agent authority remained usable after human takeover".into());
        }
        let human_authority = registry
            .authorize(
                &session.identity,
                ComputerActionActorV1::Human,
                "implementer/v1",
                unix_time_ms()?,
            )
            .map_err(|error| format!("human takeover authorization: {error:?}"))?;
        let next_action_sequence = qa_driver::run_seeded_build(
            root,
            &session,
            controller_key_id,
            &controller_key,
            &human_authority,
            &mut channel,
            &journal,
            next_action_sequence,
        )?;
        let handoff_observation = observe_frame_as(
            &human_authority,
            &mut channel,
            &journal,
            &session,
            ComputerActionActorV1::Human,
            next_action_sequence,
        )?;
        let handoff_window_id = handoff_observation
            .windows
            .iter()
            .find(|window| window.app_id.as_deref() == Some("chromium"))
            .map(|window| window.window_id.clone())
            .ok_or("Chromium window missing after QA app cleanup")?;
        let human_action = action_as(
            &session,
            ComputerActionActorV1::Human,
            next_action_sequence + 1,
            ComputerOperationV1::KeyboardInput,
            ComputerActionPayloadV1::KeyPress {
                key: ComputerKeyV1::Named(KeyCodeV1::Escape),
                window_id: handoff_window_id,
                observation_revision: handoff_observation.observation_revision,
            },
        );
        human_authority
            .execute(
                &mut channel,
                &journal,
                &human_action,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("human-controlled guest keypress: {error:?}"))?;
        session.lifecycle = ComputerLifecycleV1::Delegated;
        session
            .attest(controller_key_id.to_owned(), &controller_key)
            .map_err(str::to_owned)?;
        registry
            .replace_actor(
                session.clone(),
                controller_key_id.to_owned(),
                controller_key.verifying_key(),
                ComputerActionActorV1::Agent,
                "implementer/v1".into(),
                unix_time_ms()?,
            )
            .map_err(|error| format!("agent handback authority replacement: {error:?}"))?;
        if human_authority
            .execute(
                &mut channel,
                &journal,
                &human_action,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .is_ok()
        {
            return Err("human authority remained usable after agent handback".into());
        }

        if channel.capabilities().accessibility_tree {
            return Err("real guest unexpectedly advertised accessibility support".into());
        }
        let agent_authority = registry
            .authorize(
                &session.identity,
                ComputerActionActorV1::Agent,
                "implementer/v1",
                unix_time_ms()?,
            )
            .map_err(|error| format!("agent handback authority lookup: {error:?}"))?;
        let accessibility = action(
            &session,
            next_action_sequence + 2,
            ComputerOperationV1::ReadAccessibility,
            ComputerActionPayloadV1::ReadAccessibility,
        );
        let accessibility_result = agent_authority.execute(
            &mut channel,
            &journal,
            &accessibility,
            unix_time_ms()?.saturating_add(60_000),
            unix_time_ms()?,
        );
        if accessibility_result
            != Err(crate::computer_use::guest_protocol::GuestProtocolErrorV1::UnsupportedCapability)
        {
            return Err(format!(
                "live accessibility request was not rejected as unsupported: {accessibility_result:?}"
            ));
        }
        if journal
            .status_for_sequence(next_action_sequence + 2)
            .map_err(|error| format!("accessibility journal status: {error:?}"))?
            != crate::computer_use::guest_protocol::GuestActionJournalStatusV1::NotRecorded
        {
            return Err(
                "unsupported accessibility request was recorded in the action journal".into(),
            );
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let display_frame = loop {
            match backend.capture_display_frame(&session, &handle, unix_time_ms()?) {
                Ok(frame) if frame.width > 0 && frame.height > 0 && !frame.png.is_empty() => {
                    break frame;
                }
                Ok(_) | Err(_) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(200))
                }
                Ok(_) => return Err("QEMU display stream returned an empty frame".into()),
                Err(error) => return Err(format!("QEMU display stream: {error:?}")),
            }
        };
        peak_memory_bytes = performance::read_cgroup_value(&session_cgroup, "memory.peak")?;
        println!(
            "R1 real KVM evidence: image={} companion_protocol={} guest_frame={}x{} display_frame={}x{} image_acquire_ms={} provisioning_ms={} screen_observation_ms={} idle_sample_window_ms={} idle_memory_bytes={} idle_cpu_delta_usec={} session_peak_memory_bytes={}",
            image.entry().image_digest,
            image.entry().companion_protocol_version,
            observation.width_px,
            observation.height_px,
            display_frame.width,
            display_frame.height,
            image_download_ms,
            provisioning_ms,
            screen_observation_ms,
            idle_sample_window_ms,
            idle_memory_bytes,
            idle_cpu_usec,
            peak_memory_bytes,
        );

        if wait_for_controller_restart {
            controller_restart::publish_ready(
                root,
                &session.identity.session_id,
                session.lease.generation,
                &machine_id,
            )?;
            println!("R1 real KVM controller restart evidence: active session is ready");
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }

        // Simulate an unexpected hypervisor exit after the session has carried
        // real guest input, QA staging, takeover, and display traffic. Collect
        // the dead child, then recover its durable session record from a fresh
        // backend instance and verify that its owned resources are removed.
        let owner_path = session_directory.join("owner.json");
        let owner: serde_json::Value = serde_json::from_slice(
            &fs::read(&owner_path).map_err(|error| format!("read crash-test owner: {error}"))?,
        )
        .map_err(|error| format!("parse crash-test owner: {error}"))?;
        let qemu_pid = owner
            .get("pid")
            .and_then(serde_json::Value::as_u64)
            .ok_or("crash-test owner record has no QEMU process id")?;
        let command_line = fs::read(format!("/proc/{qemu_pid}/cmdline"))
            .map_err(|error| format!("read crash-test QEMU command line: {error}"))?;
        if !command_line
            .windows(session.identity.session_id.len())
            .any(|part| part == session.identity.session_id.as_bytes())
        {
            return Err("crash-test QEMU process did not match the active session".into());
        }
        if unsafe { libc::kill(qemu_pid as i32, libc::SIGKILL) } != 0 {
            return Err(format!(
                "could not inject the QEMU crash: {}",
                std::io::Error::last_os_error()
            ));
        }
        let collect_deadline = Instant::now() + Duration::from_secs(5);
        let crashed_record = loop {
            let record = backend
                .collect(&session, &handle, unix_time_ms()?)
                .map_err(|error| format!("collect crashed QEMU session: {error:?}"))?;
            if record.state == ComputerMachineStateV1::Stopped {
                break record;
            }
            if Instant::now() >= collect_deadline {
                return Err("crashed QEMU session was not collected within five seconds".into());
            }
            thread::sleep(Duration::from_millis(25));
        };
        if crashed_record.exit_code.is_some() {
            return Err(format!(
                "crashed QEMU unexpectedly reported a normal exit code: {:?}",
                crashed_record.exit_code
            ));
        }
        let recovery_backend = LocalKvmComputerBackend::new(
            root.join("runtime"),
            root.join("overlays"),
            cgroup_root.clone(),
            machine_id.clone(),
            controller_key_id.to_owned(),
            controller_key.verifying_key(),
            ComputerResourceLimitsV1::default(),
        )
        .map_err(|error| format!("recovery backend configuration: {error:?}"))?;
        let recovery_started = Instant::now();
        let recovered = recovery_backend
            .recover()
            .map_err(|error| format!("recover crashed QEMU session: {error:?}"))?;
        if recovered.len() != 1
            || recovered[0].session_id != session.identity.session_id
            || recovered[0].lease_generation != session.lease.generation
            || recovered[0].kind != crate::computer_use::ComputerBackendEventKindV1::Recovered
        {
            return Err(format!("unexpected crash recovery events: {recovered:?}"));
        }
        crash_recovery_ms = Some(recovery_started.elapsed().as_millis());
        println!(
            "R1 real KVM crash recovery evidence: state={:?} recovered_events={} recovery_ms={}",
            crashed_record.state,
            recovered.len(),
            crash_recovery_ms.unwrap_or_default(),
        );
        Ok(())
    })();
    let cleanup_started = Instant::now();
    if crash_recovery_ms.is_none() {
        let _ = backend.stop(&session, &handle, unix_time_ms()?);
        let cleanup = backend.release(&session, &handle, unix_time_ms()?);
        if let Err(error) = cleanup {
            return Err(format!(
                "VM cleanup: {error:?}; acceptance result: {}",
                result.err().unwrap_or_else(|| "passed".into())
            ));
        }
        cleanup::assert_released(&session_directory, &session_cgroup, &overlay_root)?;
    }
    let cleanup_ms = crash_recovery_ms.unwrap_or_else(|| cleanup_started.elapsed().as_millis());
    if crash_recovery_ms.is_some() {
        cleanup::assert_released(&session_directory, &session_cgroup, &overlay_root)?;
    }
    println!(
        "R1 real KVM cleanup evidence: cleanup_ms={} recovery_path={}",
        cleanup_ms,
        crash_recovery_ms.is_some()
    );
    result?;
    performance::assert_acceptance_budgets(performance::AcceptanceMetrics {
        provisioning_ms,
        screen_observation_ms,
        idle_sample_window_ms,
        idle_memory_bytes,
        idle_cpu_usec,
        peak_memory_bytes,
        cleanup_ms,
    })?;
    Ok(())
}

fn action(
    session: &crate::computer_use::ComputerSessionV1,
    sequence: u64,
    operation: ComputerOperationV1,
    payload: ComputerActionPayloadV1,
) -> ComputerActionV1 {
    action_as(
        session,
        ComputerActionActorV1::Agent,
        sequence,
        operation,
        payload,
    )
}

fn require_guest_action_completed(
    name: &str,
    result: &crate::computer_use::guest_protocol::GuestOperationResultV1,
) -> Result<(), String> {
    if *result == crate::computer_use::guest_protocol::GuestOperationResultV1::Completed {
        Ok(())
    } else {
        Err(format!("{name} returned {result:?}"))
    }
}

fn observe_frame(
    authority: &crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1,
    channel: &mut crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1,
    journal: &crate::computer_use::guest_protocol::GuestActionJournalV1,
    session: &crate::computer_use::ComputerSessionV1,
    sequence: u64,
) -> Result<crate::computer_use::guest_protocol::GuestObservationV1, String> {
    observe_frame_as(
        authority,
        channel,
        journal,
        session,
        ComputerActionActorV1::Agent,
        sequence,
    )
}

fn observe_frame_as(
    authority: &crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1,
    channel: &mut crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1,
    journal: &crate::computer_use::guest_protocol::GuestActionJournalV1,
    session: &crate::computer_use::ComputerSessionV1,
    actor: ComputerActionActorV1,
    sequence: u64,
) -> Result<crate::computer_use::guest_protocol::GuestObservationV1, String> {
    authority
        .execute(
            channel,
            journal,
            &action_as(
                session,
                actor,
                sequence,
                ComputerOperationV1::ObserveScreen,
                ComputerActionPayloadV1::ObserveScreen,
            ),
            unix_time_ms()?.saturating_add(60_000),
            unix_time_ms()?,
        )
        .map_err(|error| format!("observe after real guest input: {error:?}"))?
        .observation
        .ok_or_else(|| "observe after real guest input returned no frame".into())
}

fn action_as(
    session: &crate::computer_use::ComputerSessionV1,
    actor: ComputerActionActorV1,
    sequence: u64,
    operation: ComputerOperationV1,
    payload: ComputerActionPayloadV1,
) -> ComputerActionV1 {
    ComputerActionV1 {
        schema_version: 1,
        session_id: session.identity.session_id.clone(),
        action_sequence: sequence,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        actor,
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        operation,
        payload,
        helper_authorization: None,
    }
}

fn unix_time_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis()
        .try_into()
        .map_err(|_| "system clock overflow".into())
}

fn monitor_snapshot(path: &Path) -> Result<String, String> {
    let mut stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("monitor closed before prompt".into());
        }
        output.extend_from_slice(&buffer[..read]);
        if output.ends_with(b"(qemu) ") {
            break;
        }
    }
    stream
        .write_all(b"info chardev\ninfo block\ninfo status\n")
        .map_err(|error| error.to_string())?;
    output.clear();
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
        if output.ends_with(b"(qemu) ") || output.len() >= 16_384 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}
