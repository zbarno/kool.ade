mod limits;

use super::read_budget::PlanningReadBudget;
use super::{PiHarness, compress_completed_events, configured_stall_timeout, diagnostics, tail};
use crate::error::AppError;
use crate::harness::pi_events::EventFold;
use crate::harness::pi_extract::extract_json_object;
use crate::harness::pi_proc::StreamEvt;
use crate::harness::{
    AiHarness, ExecutionMode, HarnessOutcome, PlanningRequest, RetrievalPlan, ToolAccess,
};
use std::time::{Duration, Instant};

impl AiHarness for PiHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("pi {version}"),
            Err(e) => format!("pi (unavailable: {})", e.headline()),
        }
    }

    fn check_available(&self) -> Result<String, AppError> {
        let exe = Self::locate_binary()?;
        Self::check_binary(&exe)
    }

    fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let exe = Self::locate_binary()?;
        super::capabilities::validate(&exe, req.mode)?;
        let exe = exe
            .canonicalize()
            .map_err(|error| AppError::Other(format!("Cannot resolve Pi executable: {error}")))?;
        let runtime = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        if req.mode.tool_access() == ToolAccess::ReadOnly
            && !runtime.repository_planning_available()
        {
            return Err(AppError::Other(
                runtime.planning_unavailable_message().to_owned(),
            ));
        }
        let tool_access = runtime.tool_access(req.mode);
        let planning_reads = tool_access == ToolAccess::ReadOnly;
        let mut argv = vec![exe.to_string_lossy().into_owned()];
        let system_instructions = match req.mode {
            ExecutionMode::Implementation => format!(
                "{}\n\n{}",
                req.system_instructions,
                crate::harness::pi_sandbox::IMPLEMENTATION_POLICY
            ),
            mode if mode.tool_access() == ToolAccess::ReadOnly && planning_reads => format!(
                "{}\n\n{}",
                req.system_instructions,
                crate::harness::pi_sandbox::PLANNING_POLICY
            ),
            mode if mode.tool_access() == ToolAccess::ReadOnly => format!(
                "{}\n\n{}",
                req.system_instructions,
                crate::harness::pi_sandbox::PLANNING_CONTEXT_ONLY_POLICY
            ),
            _ => req.system_instructions.clone(),
        };
        argv.extend([
            "-p".into(),
            "--mode".into(),
            "json".into(),
            "--no-session".into(),
            "--no-approve".into(),
            "--no-context-files".into(),
            "--no-extensions".into(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--append-system-prompt".into(),
            format!("PROCESS OWNERSHIP — mandatory for every tool call: Kool.ad/e is the supervising application (PID {}). Never signal or terminate Kool.ad/e, its ancestors, other operator windows, or unrelated workers. Do not use pkill/killall, command-name or command-line matching, or machine-wide process sweeps to select kill targets. Test cleanup may stop only processes you launched and recorded for that test run. For detached GUI children, require a unique inherited run marker plus the exact executable and test display; verify ownership before each signal and use pidfds where available to avoid PID reuse. Inspect existing cleanup helpers before running them; repair broad process matching first. If ownership cannot be established, preserve the process and report it. A private test display alone does not isolate processes or authorize killing other app instances.\n\n{}", std::process::id(), system_instructions),
            "--thinking".into(),
            req.reasoning_level.clone(),
        ]);

        let mut _extension_files = None;
        let mut _sandbox = None;
        let mut _planning_sandbox = None;
        let mut resource_bridge = None;
        let mut git_common_dir = None;
        let mut child_env = Vec::new();
        if req.mode == ExecutionMode::Implementation {
            let bridge = crate::harness::resource_bridge::ResourceBridge::start(&req.repo_root)
                .map_err(|error| {
                    AppError::Other(format!("Cannot start resource broker: {error:#}"))
                })?;
            let mut sandbox = crate::harness::pi_sandbox::Sandbox::new_for_pi(&req.repo_root, &exe)
                .map_err(|error| {
                    AppError::Other(format!("Cannot start bounded implementation: {error:#}"))
                })?;
            sandbox
                .mount_resource_cache(bridge.cache_path())
                .map_err(|error| {
                    AppError::Other(format!("Cannot mount resource cache: {error:#}"))
                })?;
            sandbox
                .mount_npm_cache(bridge.npm_cache_path(), true)
                .map_err(|error| {
                    AppError::Other(format!("Cannot mount prepared npm cache: {error:#}"))
                })?;
            let files = sandbox.extension_files().map_err(|error| {
                AppError::Other(format!(
                    "Cannot prepare bounded implementation tools: {error:#}"
                ))
            })?;
            let config = sandbox.extension_config().map_err(|error| {
                AppError::Other(format!(
                    "Cannot configure bounded implementation tools: {error:#}"
                ))
            })?;
            argv.extend([
                "--no-builtin-tools".into(),
                "--tools".into(),
                "koolade_bash,koolade_resource".into(),
                "--extension".into(),
                files.extension.to_string_lossy().into_owned(),
            ]);
            child_env.push(("KOOLADE_SANDBOX_CONFIG".into(), config));
            child_env.push((
                "KOOLADE_RESOURCE_SOCKET".into(),
                bridge.socket_path().to_string_lossy().into_owned(),
            ));
            git_common_dir = Some(sandbox.git_common_dir.clone());
            _extension_files = Some(files);
            _sandbox = Some(sandbox);
            resource_bridge = Some(bridge);
        } else {
            match tool_access {
                ToolAccess::None => argv.push("--no-tools".into()),
                ToolAccess::ReadOnly => {
                    if planning_reads {
                        argv.extend(["--tools".into(), "read,grep,find,ls".into()]);
                        let sandbox =
                            crate::harness::pi_sandbox::PlanningSandbox::new(&req.repo_root, &exe)
                                .map_err(|error| {
                                    AppError::Other(format!(
                                        "Cannot start bounded planning reads: {error:#}"
                                    ))
                                })?;
                        let mut wrapped = vec![sandbox.bwrap.to_string_lossy().into_owned()];
                        wrapped.extend(sandbox.command_args(&argv));
                        argv = wrapped;
                        _planning_sandbox = Some(sandbox);
                    } else {
                        argv.push("--no-tools".into());
                    }
                }
                ToolAccess::BoundedImplementation => {
                    return Err(AppError::Other(
                        "Implementation mode did not initialize its bounded tool sandbox".into(),
                    ));
                }
            }
        }
        if req.mode == ExecutionMode::Implementation {
            argv.retain(|arg| arg != "--no-context-files");
        }
        let mut diagnostics =
            diagnostics::open(&req.repo_root, req.mode, git_common_dir.as_deref())?;
        let task = crate::harness::pi_proc::spawn_with_input_env(
            &argv,
            &req.repo_root,
            Some(req.prompt_body.clone()),
            &child_env,
        )?;
        let deadline = Instant::now() + req.timeout;
        let stall_limit = configured_stall_timeout();
        let stall_secs = stall_limit.as_secs();
        let mut last_output = Instant::now();
        let mut fold = EventFold::default();
        let mut stderr_tail: Vec<String> = Vec::new();
        let mut last_preview = crate::harness::LiveProgress::default();
        let mut last_emit = Instant::now() - Duration::from_millis(50);
        let mut preview_dirty = false;
        let mut planning_read_budget = PlanningReadBudget::default();

        loop {
            if req.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                task.kill();
                let _ = task.settle(Duration::from_secs(3));
                return Err(AppError::HarnessFailed {
                    reason: "cancelled by user".into(),
                    stderr_tail: tail(&stderr_tail),
                });
            }
            if Instant::now() >= deadline {
                task.kill();
                let _ = task.settle(Duration::from_secs(3));
                return Err(AppError::HarnessTimedOut {
                    secs: req.timeout.as_secs(),
                });
            }
            if preview_dirty && last_emit.elapsed() >= Duration::from_millis(50) {
                let preview = fold.preview();
                if preview != last_preview {
                    let _ = req.progress_tx.send(preview.clone());
                    last_preview = preview;
                }
                preview_dirty = false;
                last_emit = Instant::now();
            }
            // NOTE: `Pending` is NOT an error state — the first event from a
            // cold harness can lag well past one poll window.
            match task.poll_next(Duration::from_millis(200)) {
                Err(crate::harness::pi_proc::PollState::Pending) => {
                    if last_output.elapsed() >= stall_limit {
                        task.kill();
                        let _ = task.settle(Duration::from_secs(3));
                        return Err(AppError::HarnessFailed {
                            reason: format!(
                                "harness stalled: pi produced no output for {stall_secs}s and was presumed hung"
                            ),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    continue;
                }
                Ok(StreamEvt::Stdout(line)) => {
                    last_output = Instant::now();
                    if let Some((_, file)) = diagnostics.as_mut() {
                        use std::io::Write;
                        writeln!(file, "{line}").map_err(|e| {
                            AppError::Other(format!("Cannot write harness diagnostics: {e}"))
                        })?;
                    }
                    if planning_reads && let Err(reason) = planning_read_budget.observe_line(&line)
                    {
                        return Err(limits::fail(
                            &task,
                            diagnostics.as_ref(),
                            reason,
                            &req.progress_tx,
                            &stderr_tail,
                        ));
                    }
                    crate::harness::pi_events::fold_line(&line, &mut fold);
                    preview_dirty = true;
                }
                Ok(StreamEvt::Stderr(line)) => {
                    last_output = Instant::now();
                    stderr_tail.push(line);
                    if stderr_tail.len() > 100 {
                        stderr_tail.remove(0);
                    }
                }
                Ok(StreamEvt::Exited(ok)) => {
                    if let Some(error) = resource_attention(resource_bridge.as_ref()) {
                        return Err(error);
                    }
                    if !ok {
                        return Err(AppError::HarnessFailed {
                            reason: fold
                                .error_hint
                                .clone()
                                .unwrap_or_else(|| "pi exited with a failure code".into()),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    break;
                }
                Err(crate::harness::pi_proc::PollState::Closed) => {
                    if let Some(error) = resource_attention(resource_bridge.as_ref()) {
                        return Err(error);
                    }
                    // Pipe disconnected without an Exited event (defensive).
                    if !fold.saw_agent_end && fold.final_assistant_text.is_empty() {
                        return Err(AppError::HarnessFailed {
                            reason: "stream ended unexpectedly".into(),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    break;
                }
            }
        }

        let _ = req.progress_tx.send(fold.preview());
        if let Some(error) = resource_attention(resource_bridge.as_ref()) {
            return Err(error);
        }
        let final_text = std::mem::take(&mut fold.final_assistant_text);
        if final_text.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: format!(
                    "pi finished but produced no final assistant message ({} parsed events, {} unparsed lines, agent_end={}, last_stop_reason={}, last_error_class={}, tool_executions={}; diagnostics: {})",
                    fold.events_seen,
                    fold.unparsed_lines,
                    fold.saw_agent_end,
                    fold.last_stop_reason.as_deref().unwrap_or("unknown"),
                    fold.last_error_class.unwrap_or("unknown"),
                    fold.tool_executions,
                    diagnostics
                        .as_ref()
                        .map(|(path, _)| path.display().to_string())
                        .unwrap_or_else(|| "not recorded (no writable Git metadata)".into())
                ),
                stderr_tail: tail(&stderr_tail),
            });
        }
        // Keep a raw stream through the run so a crash or failed invocation
        // leaves a readable diagnostic at the path named in its error. Once a
        // final answer exists, archive that stream losslessly. Compression is
        // best effort: the raw file remains if gzip is unavailable or fails.
        if let Some((path, file)) = diagnostics.take() {
            drop(file);
            let _ = compress_completed_events(&path);
        }
        Ok(HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: tail(&stderr_tail),
        })
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        let outcome = self.execute(request)?;
        let object = extract_json_object(&outcome.final_text)
            .ok_or_else(|| AppError::Other("Retrieval planner returned no JSON object".into()))?;
        serde_json::from_str(&object)
            .map(Some)
            .map_err(|error| AppError::Other(format!("Invalid retrieval plan: {error}")))
    }
}

fn resource_attention(
    bridge: Option<&crate::harness::resource_bridge::ResourceBridge>,
) -> Option<AppError> {
    bridge?.attention_detail().map(|detail| AppError::Other(format!(
        "Needs Attention: the worker requested a resource that requires operator review. {detail}"
    )))
}
