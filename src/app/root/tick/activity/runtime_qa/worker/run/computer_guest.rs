use std::{
    collections::BTreeMap,
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crate::{
    app::root::tick::activity::runtime_qa::ComputerQaRuntimeContextV1,
    computer_use::mcp::{
        ComputerUseMcpRequestV1 as Request, ComputerUseMcpStatusV1 as Status,
        ComputerUseObservationKindV1 as ObservationKind,
    },
    computer_use::{
        ComputerActionPayloadV1, ComputerFrameReferenceV1, ComputerKeyV1, MouseButtonV1,
        apps::{GuestAppArchitectureV1, StagedTaskApplicationBuildV1, StagedTaskBuildFileV1},
    },
    runtime_qa::{
        QaComputerBuildApprovalV1, QaComputerStagedArtifactV1, QaComputerTargetV1,
        QaDriverActionV1, QaExecutionTargetV1, QaRunRequestV1, QaScenarioV1, QaTargetAuthorization,
        RuntimeQaStore,
    },
};

use super::PreparedRequest;

#[allow(clippy::too_many_arguments)] // Keep approval, lifecycle claim, selected build, reviewer, cancellation, and consent capability explicit.
pub(super) fn run(
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
    approval_revision: u64,
    claimed_task_revision: u64,
    scenarios: &[QaScenarioV1],
    upload: &crate::execution::docker::qa_container::QaContainerUpload,
    harness: Box<dyn crate::harness::AiHarness>,
    store: &RuntimeQaStore,
    cancelled: Arc<AtomicBool>,
    context: ComputerQaRuntimeContextV1,
) -> Result<(), String> {
    let approval = store.load_computer_build_approval(request, approval_revision)?;
    approval.validate_for(
        request,
        approval_revision,
        QaExecutionTargetV1::ComputerUseGuest,
    )?;
    let session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let caller = context.caller(&request.parent_task_uid, &session_id);
    caller.identity.validate().map_err(str::to_owned)?;
    let purpose = format!(
        "Computer QA run {} task {} source {} build {} approval {}",
        request.qa_run_id,
        request.parent_task_uid,
        approval.revision_sha,
        approval.build_receipt_sha256,
        approval.approval_sha256()?,
    );
    let requested = context
        .host
        .handle(&caller, Request::RequestSession { purpose }, &cancelled);
    if requested.status != Status::Ready || requested.session_id.as_deref() != Some(&session_id) {
        return Err(
            "Computer QA requires explicit approval in the Computer Use consent card".into(),
        );
    }
    let started = context.host.handle(
        &caller,
        Request::StartSession {
            session_id: session_id.clone(),
        },
        &cancelled,
    );
    if started.status != Status::Completed {
        let cleanup_cancelled = AtomicBool::new(false);
        let _ = context.host.handle(
            &caller,
            Request::Release {
                session_id: session_id.clone(),
            },
            &cleanup_cancelled,
        );
        return Err("approved Computer QA disposable guest session did not become ready".into());
    }
    let result = run_in_session(
        prepared,
        request,
        approval_revision,
        claimed_task_revision,
        &approval,
        scenarios,
        upload,
        harness,
        store,
        cancelled.clone(),
        &context,
        &caller,
    );
    let cleanup_cancelled = AtomicBool::new(false);
    let release = context
        .host
        .handle(&caller, Request::Release { session_id }, &cleanup_cancelled);
    match (result, release.status) {
        (Ok(()), Status::Completed) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), _) => Err("Computer QA guest session could not be safely released".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_in_session(
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
    approval_revision: u64,
    claimed_task_revision: u64,
    approval: &QaComputerBuildApprovalV1,
    scenarios: &[QaScenarioV1],
    upload: &crate::execution::docker::qa_container::QaContainerUpload,
    harness: Box<dyn crate::harness::AiHarness>,
    store: &RuntimeQaStore,
    cancelled: Arc<AtomicBool>,
    context: &ComputerQaRuntimeContextV1,
    caller: &crate::computer_use::mcp::ComputerUseCallerV1,
) -> Result<(), String> {
    ensure_current_task(prepared, request, approval_revision, claimed_task_revision)?;
    let image_digest = context
        .controller
        .computer_qa_session_guest_image_digest(
            &request.parent_task_uid,
            &caller.identity.session_id,
        )
        .map_err(str::to_owned)?;
    let attestation = context
        .controller
        .computer_qa_session_attestation_sha256(
            &request.parent_task_uid,
            &caller.identity.session_id,
        )
        .map_err(str::to_owned)?;
    let mut guest = HostGuestV1 {
        context,
        caller,
        latest: None,
        request,
        approval,
        approval_revision,
        prepared,
        claimed_task_revision,
        cancelled: &cancelled,
    };
    let initial = guest.observe(ObservationKind::Screenshot)?;
    let width =
        u16::try_from(initial.width_px).map_err(|_| "guest display width is out of bounds")?;
    let height =
        u16::try_from(initial.height_px).map_err(|_| "guest display height is out of bounds")?;
    let dpi = observed_dpi(&initial)?;
    let executable_path = target_executable_path(&prepared.startup_recipe)?;
    let files = extract_files(upload, &prepared.artifact_paths)?;
    let build = StagedTaskApplicationBuildV1 {
        schema_version: 1,
        app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
        executable_id: request
            .target_id
            .clone()
            .ok_or("Computer QA request has no approved app target")?,
        executable_relative_path: executable_path.clone(),
        app_target_id: request
            .target_id
            .clone()
            .ok_or("Computer QA request has no approved app target")?,
        execution_recipe_id: request.startup_recipe_ref.clone(),
        build_receipt_sha256: request.build_artifact_sha256.clone(),
        guest_image_digest: image_digest.clone(),
        guest_architecture: GuestAppArchitectureV1::X86_64,
        files,
    };
    ensure_current_task(prepared, request, approval_revision, claimed_task_revision)?;
    let staged = context
        .controller
        .stage_approved_task_application(
            &caller.identity.session_id,
            request,
            approval,
            approval_revision,
            build,
            &cancelled,
        )
        .map_err(str::to_owned)?;
    let target = QaComputerTargetV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        identity: caller.identity.clone(),
        application_id: staged.app_id.clone(),
        executable_id: staged.executable_id.clone(),
        app_target_id: staged.app_target_id.clone(),
        execution_recipe_id: staged.execution_recipe_id.clone(),
        execution_recipe_sha256: approval.execution_recipe_sha256.clone(),
        relative_executable_path: staged.executable_relative_path.clone(),
        guest_image_digest: image_digest,
        guest_architecture: "x86_64".into(),
        session_attestation_sha256: attestation,
        build_approval_sha256: staged.build_approval_sha256.clone(),
        task_revision: approval.revision_sha.clone(),
        approved_spec_hash: approval.approved_spec_hash.clone(),
        display_width: width,
        display_height: height,
        display_dpi: dpi,
        staged_artifacts: staged
            .artifacts
            .iter()
            .map(|file| QaComputerStagedArtifactV1 {
                relative_path: file.relative_path.clone(),
                transfer_grant_id: file.transfer_grant_id.clone(),
                receipt: file.receipt.clone(),
            })
            .collect(),
    };
    target.validate_for(request)?;
    let driver = crate::runtime_qa::drivers::computer_vm::ComputerQaScenarioDriverV1 {
        request,
        target: &target,
        guest: &mut guest,
    };
    super::run_reviewed(
        request,
        scenarios,
        QaTargetAuthorization::ComputerVm(&target),
        driver,
        harness,
        &prepared.workspace,
        &prepared.task_clone,
        store,
        cancelled.clone(),
    )
}

fn target_executable_path(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> Result<String, String> {
    if !crate::project_runtime::is_target_relative_executable_path(&recipe.executable_id) {
        return Err("Computer QA only stages a target-relative executable; logical tool recipes are unsupported".into());
    }
    let path = if recipe.working_directory_relative == "." {
        recipe.executable_id.clone()
    } else {
        format!(
            "{}/{}",
            recipe.working_directory_relative, recipe.executable_id
        )
    };
    if !path
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
    {
        return Err("Computer QA executable path is not a safe relative artifact path".into());
    }
    Ok(path)
}

fn observed_dpi(
    observation: &crate::computer_use::guest_protocol::GuestObservationV1,
) -> Result<u16, String> {
    let dpi = 96_u32
        .checked_mul(observation.device_scale_numerator)
        .and_then(|scaled| scaled.checked_div(observation.device_scale_denominator))
        .ok_or_else(|| "guest display scale is invalid".to_owned())?;
    u16::try_from(dpi).map_err(|_| "guest display scale is out of bounds".into())
}

fn extract_files(
    upload: &crate::execution::docker::qa_container::QaContainerUpload,
    paths: &[String],
) -> Result<Vec<StagedTaskBuildFileV1>, String> {
    let allowed = paths
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut archive = tar::Archive::new(upload.archive.as_slice());
    let mut files = BTreeMap::new();
    for entry in archive
        .entries()
        .map_err(|_| "approved QA artifact snapshot could not be read")?
    {
        let mut entry = entry.map_err(|_| "approved QA artifact entry is malformed")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|_| "approved QA artifact path is malformed")?
            .to_string_lossy()
            .replace('\\', "/");
        if !allowed.contains(&path) || files.contains_key(&path) {
            return Err("approved QA artifact snapshot contains an unexpected path".into());
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| "approved QA artifact bytes could not be read")?;
        files.insert(path, bytes);
    }
    if files.len() != allowed.len() {
        return Err("approved QA artifact snapshot is incomplete".into());
    }
    Ok(files
        .into_iter()
        .map(|(relative_path, bytes)| StagedTaskBuildFileV1 {
            relative_path,
            bytes,
        })
        .collect())
}

fn ensure_current_task(
    prepared: &PreparedRequest,
    request: &QaRunRequestV1,
    approval_revision: u64,
    claimed_task_revision: u64,
) -> Result<(), String> {
    let task = crate::core::lifecycle::LifecycleStore::new(&prepared.planning_store)
        .load_task(&request.parent_task_uid)
        .map_err(|error| format!("Computer QA lifecycle task could not be revalidated: {error}"))?;
    let execution = task.qa_execution.as_ref();
    if task.revision != claimed_task_revision
        || task.phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
        || execution.is_none_or(|execution| {
            execution.status != crate::core::lifecycle::QaExecutionStatusV1::Running
                || execution.request.as_ref() != Some(request)
                || execution.dispatch_task_revision != Some(approval_revision)
        })
    {
        return Err("Computer QA task, request, revision, or dispatch claim changed; guest access was stopped".into());
    }
    Ok(())
}

struct HostGuestV1<'a> {
    context: &'a ComputerQaRuntimeContextV1,
    caller: &'a crate::computer_use::mcp::ComputerUseCallerV1,
    latest: Option<crate::computer_use::guest_protocol::GuestObservationV1>,
    request: &'a QaRunRequestV1,
    approval: &'a QaComputerBuildApprovalV1,
    approval_revision: u64,
    prepared: &'a PreparedRequest,
    claimed_task_revision: u64,
    cancelled: &'a AtomicBool,
}

impl HostGuestV1<'_> {
    fn ensure_current_task(&self) -> Result<(), String> {
        ensure_current_task(
            self.prepared,
            self.request,
            self.approval_revision,
            self.claimed_task_revision,
        )
    }
    fn observe(
        &mut self,
        kind: ObservationKind,
    ) -> Result<crate::computer_use::guest_protocol::GuestObservationV1, String> {
        self.ensure_current_task()?;
        let response = self.context.host.handle(
            self.caller,
            Request::Observe {
                session_id: self.caller.identity.session_id.clone(),
                observation: kind,
            },
            self.cancelled,
        );
        if response.status != Status::Completed {
            return Err("Computer QA guest observation failed".into());
        }
        let observation = response
            .observation
            .ok_or_else(|| "Computer QA guest observation was missing".to_owned())?;
        self.latest = Some(observation.clone());
        Ok(observation)
    }

    fn action(&mut self, payload: ComputerActionPayloadV1) -> Result<(), String> {
        self.ensure_current_task()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err("Computer QA was cancelled".into());
        }
        let response = self.context.host.handle(
            self.caller,
            Request::Action {
                session_id: self.caller.identity.session_id.clone(),
                payload,
                timeout_ms: 60_000,
                idempotency_key: None,
            },
            self.cancelled,
        );
        if response.status == Status::Completed {
            Ok(())
        } else {
            Err("Computer QA guest action failed".into())
        }
    }

    fn window(
        &self,
        id: &str,
    ) -> Result<crate::computer_use::guest_protocol::GuestWindowObservationV1, String> {
        self.latest.as_ref().and_then(|observation| observation.windows.iter().find(|window| window.window_id == id)).cloned().ok_or_else(|| "Computer QA action referenced a window outside the latest authenticated observation".into())
    }

    fn frame(&self, window_id: &str) -> Result<ComputerFrameReferenceV1, String> {
        let observation = self
            .latest
            .as_ref()
            .ok_or_else(|| "Computer QA requires a fresh guest observation".to_owned())?;
        let window = self.window(window_id)?;
        Ok(ComputerFrameReferenceV1 {
            frame_id: observation.frame_id.clone(),
            display_generation: observation.display_generation,
            observation_revision: observation.observation_revision,
            display_width_px: observation.width_px,
            display_height_px: observation.height_px,
            device_scale_numerator: observation.device_scale_numerator,
            device_scale_denominator: observation.device_scale_denominator,
            window_id: window.window_id,
            window_generation: window.generation,
            window_bounds: window.bounds,
        })
    }
}

impl crate::runtime_qa::drivers::computer_vm::ComputerQaGuestSessionV1 for HostGuestV1<'_> {
    fn begin_scenario(
        &mut self,
        _: &QaComputerTargetV1,
        _: &str,
        deadline: Instant,
    ) -> Result<crate::runtime_qa::drivers::computer_vm::ComputerQaDisplayV1, String> {
        if Instant::now() >= deadline {
            return Err("Computer QA scenario timed out".into());
        }
        let observation = self.observe(ObservationKind::Screenshot)?;
        Ok(
            crate::runtime_qa::drivers::computer_vm::ComputerQaDisplayV1 {
                width: observation.width_px as u16,
                height: observation.height_px as u16,
                dpi: observed_dpi(&observation)?,
            },
        )
    }
    fn launch_staged_application(
        &mut self,
        target: &QaComputerTargetV1,
        _: Instant,
    ) -> Result<(), String> {
        self.ensure_current_task()?;
        self.context
            .controller
            .launch_approved_computer_qa_target(
                self.request,
                self.approval,
                self.approval_revision,
                target.clone(),
                self.cancelled,
            )
            .map(|_| ())
            .map_err(|_| "approved Computer QA application launch failed".into())
    }
    fn observe_state(&mut self, _: Instant) -> Result<String, String> {
        let observation = self.observe(ObservationKind::Windows)?;
        Ok(observation
            .windows
            .iter()
            .map(|window| {
                format!(
                    "{} {}",
                    window.title,
                    window.app_id.as_deref().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(" | "))
    }
    fn perform_action(
        &mut self,
        action: &QaDriverActionV1,
        deadline: Instant,
    ) -> Result<(), String> {
        use QaDriverActionV1 as A;
        match action {
            A::WaitForWindow => loop {
                let observation = self.observe(ObservationKind::Windows)?;
                if !observation.windows.is_empty() {
                    return Ok(());
                }
                if self.cancelled.load(Ordering::Acquire) {
                    return Err("Computer QA was cancelled".into());
                }
                if Instant::now() >= deadline {
                    return Err("Computer QA timed out waiting for a guest window".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            },
            A::ListWindows => {
                self.observe(ObservationKind::Windows)?;
                Ok(())
            }
            A::FocusWindow { window_id } => {
                let window = self.window(window_id)?;
                self.action(ComputerActionPayloadV1::FocusWindow {
                    window_id: window.window_id,
                    observation_revision: self.latest.as_ref().unwrap().observation_revision,
                })
            }
            A::Click { window_id, x, y } | A::DoubleClick { window_id, x, y } => {
                let frame = self.frame(window_id)?;
                let bounds = &frame.window_bounds;
                let x = (bounds.left.max(0) as u32).saturating_add(u32::from(*x));
                let y = (bounds.top.max(0) as u32).saturating_add(u32::from(*y));
                if matches!(action, A::Click { .. }) {
                    self.action(ComputerActionPayloadV1::MouseClick {
                        target: frame,
                        x,
                        y,
                        button: MouseButtonV1::Primary,
                    })
                } else {
                    self.action(ComputerActionPayloadV1::MouseDoubleClick {
                        target: frame,
                        x,
                        y,
                        button: MouseButtonV1::Primary,
                    })
                }
            }
            A::Drag {
                window_id,
                start_x,
                start_y,
                end_x,
                end_y,
            } => {
                let frame = self.frame(window_id)?;
                let left = frame.window_bounds.left.max(0) as u32;
                let top = frame.window_bounds.top.max(0) as u32;
                self.action(ComputerActionPayloadV1::MouseDrag {
                    target: frame,
                    from_x: left.saturating_add(u32::from(*start_x)),
                    from_y: top.saturating_add(u32::from(*start_y)),
                    to_x: left.saturating_add(u32::from(*end_x)),
                    to_y: top.saturating_add(u32::from(*end_y)),
                    button: MouseButtonV1::Primary,
                })
            }
            A::TypeText { text } => {
                let window = self
                    .latest
                    .as_ref()
                    .and_then(|o| o.windows.iter().find(|w| w.focused))
                    .cloned()
                    .ok_or_else(|| "Computer QA typing requires a focused window".to_owned())?;
                self.action(ComputerActionPayloadV1::TypeText {
                    text: text.clone(),
                    window_id: window.window_id,
                    observation_revision: self.latest.as_ref().unwrap().observation_revision,
                })
            }
            A::KeyChord { keys } => {
                let window = self
                    .latest
                    .as_ref()
                    .and_then(|o| o.windows.iter().find(|w| w.focused))
                    .cloned()
                    .ok_or_else(|| "Computer QA key input requires a focused window".to_owned())?;
                let keys = keys
                    .iter()
                    .map(|key| parse_key(key))
                    .collect::<Result<Vec<_>, _>>()?;
                self.action(ComputerActionPayloadV1::KeyChord {
                    keys,
                    window_id: window.window_id,
                    observation_revision: self.latest.as_ref().unwrap().observation_revision,
                })
            }
            A::Scroll {
                window_id,
                delta_x,
                delta_y,
            } => {
                let frame = self.frame(window_id)?;
                let x = (frame.window_bounds.left.max(0) as u32)
                    .saturating_add(frame.window_bounds.width_px / 2);
                let y = (frame.window_bounds.top.max(0) as u32)
                    .saturating_add(frame.window_bounds.height_px / 2);
                self.action(ComputerActionPayloadV1::MouseScroll {
                    target: frame,
                    x,
                    y,
                    delta_x: *delta_x,
                    delta_y: *delta_y,
                })
            }
            _ => Err(
                "Computer QA action is outside the generic authenticated GUI operation set".into(),
            ),
        }
    }
    fn screenshot_png(&mut self, _: Instant) -> Result<Vec<u8>, String> {
        let observation = self.observe(ObservationKind::Screenshot)?;
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(observation.screenshot_png_base64)
            .map_err(|_| "Computer QA guest screenshot encoding is invalid".into())
    }
    fn end_scenario(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn parse_key(value: &str) -> Result<ComputerKeyV1, String> {
    use crate::computer_use::KeyCodeV1 as K;
    Ok(match value.to_ascii_lowercase().as_str() {
        "enter" => ComputerKeyV1::Named(K::Enter),
        "escape" => ComputerKeyV1::Named(K::Escape),
        "tab" => ComputerKeyV1::Named(K::Tab),
        "backspace" => ComputerKeyV1::Named(K::Backspace),
        "delete" => ComputerKeyV1::Named(K::Delete),
        "up" => ComputerKeyV1::Named(K::ArrowUp),
        "down" => ComputerKeyV1::Named(K::ArrowDown),
        "left" => ComputerKeyV1::Named(K::ArrowLeft),
        "right" => ComputerKeyV1::Named(K::ArrowRight),
        "home" => ComputerKeyV1::Named(K::Home),
        "end" => ComputerKeyV1::Named(K::End),
        "pageup" => ComputerKeyV1::Named(K::PageUp),
        "pagedown" => ComputerKeyV1::Named(K::PageDown),
        "space" => ComputerKeyV1::Named(K::Space),
        "ctrl" | "control" => ComputerKeyV1::Named(K::Control),
        "shift" => ComputerKeyV1::Named(K::Shift),
        "alt" => ComputerKeyV1::Named(K::Alt),
        "super" | "meta" => ComputerKeyV1::Named(K::Super),
        _ if value.chars().count() == 1 => ComputerKeyV1::Character(value.chars().next().unwrap()),
        _ => return Err("Computer QA key is unsupported".into()),
    })
}

#[cfg(test)]
#[test]
fn computer_qa_only_accepts_target_relative_executable_paths() {
    let recipe = |executable: &str| crate::project_runtime::ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "start-v1".into(),
        source: crate::project_runtime::RecipeSource::ValidatedProjectConfig,
        working_directory_relative: "target/app".into(),
        executable_id: executable.into(),
        argv: Vec::new(),
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 30,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: crate::project_runtime::NetworkRequirement::Denied,
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        target_executable_path(&recipe("dist/app")).unwrap(),
        "target/app/dist/app"
    );
    assert!(target_executable_path(&recipe("npm")).is_err());
    assert!(target_executable_path(&recipe("../outside")).is_err());
}

#[cfg(test)]
#[test]
fn artifact_tar_extraction_requires_the_exact_approved_relative_path_set() {
    let mut archive = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut archive);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(3);
        header.set_mode(0o444);
        header.set_cksum();
        builder
            .append_data(&mut header, "target/app", &b"app"[..])
            .unwrap();
        builder.finish().unwrap();
    }
    let upload = crate::execution::docker::qa_container::QaContainerUpload {
        target: "/workspace".into(),
        archive,
    };
    let files = extract_files(&upload, &["target/app".into()]).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].bytes, b"app");
    assert!(extract_files(&upload, &["other/app".into()]).is_err());
}
