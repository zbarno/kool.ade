use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerBackend,
    ComputerKeyV1, ComputerLifecycleV1, ComputerNetworkProfileV1, ComputerOperationV1, KeyCodeV1,
    MINIMUM_DISPOSABLE_VM_BUDGET_V1, MouseButtonV1,
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
#[path = "r1_real_kvm/qmp.rs"]
mod qmp;
use cgroup::current_cgroup_root;

const REAL_KVM_ENV: &str = "KOOLADE_COMPUTER_USE_R1_REAL_KVM";
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
/// a command-builder unit test cannot detect QEMU-added runtime devices.
#[test]
fn computer_use_r1_real_kvm_acceptance() {
    if std::env::var(REAL_KVM_ENV).as_deref() != Ok("1") {
        return;
    }
    run_real_kvm_acceptance()
        .unwrap_or_else(|reason| panic!("R1 real KVM acceptance failed: {reason}"));
}

fn run_real_kvm_acceptance() -> Result<(), String> {
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let root = temporary.path();
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
    let image = cache
        .download_approved_catalog_release(
            MANIFEST,
            &trust,
            &floor,
            ComputerGuestArchitectureV1::X86_64,
            &TestImageApproval,
        )
        .map_err(|error| format!("approved image download and verification: {error}"))?;

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
    session.allowed_apps = BTreeSet::from(["chromium".to_owned()]);
    session.policy.allowed_apps = session.allowed_apps.clone();
    session.policy.authorized_operations = BTreeSet::from([
        ComputerOperationV1::ObserveScreen,
        ComputerOperationV1::MouseInput,
        ComputerOperationV1::KeyboardInput,
        ComputerOperationV1::LaunchAllowedApp,
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
        machine_id,
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

    let (handle, _) = backend
        .prepare(&session, overlay, unix_time_ms()?)
        .map_err(|error| format!("backend prepare: {error:?}"))?;
    let result = (|| {
        backend
            .boot(&session, &handle, unix_time_ms()?)
            .map_err(|error| format!("QEMU/KVM boot: {error:?}"))?;
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
        qmp::assert_no_network_device(
            &root
                .join("runtime/sessions")
                .join(&session.identity.session_id)
                .join("qemu-qmp.sock"),
        )
        .map_err(|error| format!("disabled-network QEMU device graph: {error}"))?;

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

        let observe = action(
            &session,
            2,
            ComputerOperationV1::ObserveScreen,
            ComputerActionPayloadV1::ObserveScreen,
        );
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
        authority
            .execute(
                &mut channel,
                &journal,
                &click,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest mouse click: {error:?}"))?;
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
        authority
            .execute(
                &mut channel,
                &journal,
                &focus_address,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest keyboard shortcut: {error:?}"))?;
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
        authority
            .execute(
                &mut channel,
                &journal,
                &address,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest text input: {error:?}"))?;
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
        authority
            .execute(
                &mut channel,
                &journal,
                &enter,
                unix_time_ms()?.saturating_add(60_000),
                unix_time_ms()?,
            )
            .map_err(|error| format!("real guest Enter key: {error:?}"))?;
        let input_frame = observe_frame(&authority, &mut channel, &journal, &session, 10)?;
        if input_frame.screenshot_sha256 == observation.screenshot_sha256 {
            return Err("real guest mouse and keyboard input did not change the screen".into());
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
        let human_action = action_as(
            &session,
            ComputerActionActorV1::Human,
            11,
            ComputerOperationV1::KeyboardInput,
            ComputerActionPayloadV1::KeyPress {
                key: ComputerKeyV1::Named(KeyCodeV1::Escape),
                window_id: window_id.clone(),
                observation_revision: input_frame.observation_revision,
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
        println!(
            "R1 real KVM evidence: image={} companion_protocol={} guest_frame={}x{} display_frame={}x{}",
            image.entry().image_digest,
            image.entry().companion_protocol_version,
            observation.width_px,
            observation.height_px,
            display_frame.width,
            display_frame.height,
        );
        Ok(())
    })();
    let _ = backend.stop(&session, &handle, unix_time_ms()?);
    let cleanup = backend.release(&session, &handle, unix_time_ms()?);
    if let Err(error) = cleanup {
        return Err(format!(
            "VM cleanup: {error:?}; acceptance result: {}",
            result.err().unwrap_or_else(|| "passed".into())
        ));
    }
    result
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

fn observe_frame(
    authority: &crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1,
    channel: &mut crate::computer_use::guest_protocol::AuthenticatedGuestChannelV1,
    journal: &crate::computer_use::guest_protocol::GuestActionJournalV1,
    session: &crate::computer_use::ComputerSessionV1,
    sequence: u64,
) -> Result<crate::computer_use::guest_protocol::GuestObservationV1, String> {
    authority
        .execute(
            channel,
            journal,
            &action(
                session,
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
