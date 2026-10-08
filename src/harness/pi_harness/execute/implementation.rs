use super::super::{PiHarness, diagnostics};
use crate::error::AppError;
use crate::harness::pi_extract::extract_json_object;
use crate::harness::{
    AiHarness, ExecutionMode, HarnessOutcome, PlanningRequest, RetrievalPlan, ToolAccess,
};

mod stream;

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
        self.execute_with_model(req, None)
    }

    fn execute_with_model(
        &self,
        req: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<HarnessOutcome, AppError> {
        let exe = Self::locate_binary()?;
        super::super::capabilities::validate(&exe, req.mode)?;
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
        if let Some(model) = model {
            let available =
                crate::harness::pi_sandbox::configured_provider_models().map_err(|error| {
                    AppError::Other(format!("Cannot validate selected Pi model: {error:#}"))
                })?;
            if !available.iter().any(|available| available == model) {
                return Err(AppError::Other(format!(
                    "Selected Pi model '{model}' is not in the configured provider's model catalog. Refresh Coding tools and choose an available model."
                )));
            }
            argv.extend(["--model".into(), model.into()]);
        }
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
            let bridge =
                crate::harness::resource_bridge::ResourceBridge::start_for_task_repository(
                    &req.repo_root,
                    req.runtime_config_source.as_deref(),
                    req.task_id.as_deref(),
                    req.progress_tx.clone(),
                    req.cancel.clone(),
                )
                .map_err(|error| {
                    AppError::Other(format!("Cannot start resource broker: {error:#}"))
                })?;
            let sandbox = match req.runtime_config_source.as_deref() {
                Some(source) => crate::harness::pi_sandbox::Sandbox::new_for_pi_task_repository(
                    &req.repo_root,
                    &exe,
                    source,
                ),
                None => crate::harness::pi_sandbox::Sandbox::new_for_pi(&req.repo_root, &exe),
            };
            let mut sandbox = sandbox.map_err(|error| {
                AppError::Other(format!("Cannot start bounded implementation: {error:#}"))
            })?;
            sandbox
                .mount_resource_cache(bridge.cache_path())
                .map_err(|error| {
                    AppError::Other(format!("Cannot mount resource cache: {error:#}"))
                })?;
            sandbox
                .mount_npm_cache_with_snapshot(
                    bridge.npm_cache_path(),
                    bridge.npm_index_snapshot_path(),
                )
                .map_err(|error| {
                    AppError::Other(format!("Cannot mount prepared npm cache: {error:#}"))
                })?;
            sandbox
                .mount_cargo_cache(bridge.cargo_cache_path())
                .map_err(|error| {
                    AppError::Other(format!("Cannot mount prepared Cargo cache: {error:#}"))
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
                "koolade_bash,koolade_resource,koolade_dependency".into(),
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
                        let sandbox = crate::harness::pi_sandbox::PlanningSandbox::new_with_model(
                            &req.repo_root,
                            &exe,
                            model,
                        )
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
        let diagnostics = diagnostics::open(&req.repo_root, req.mode, git_common_dir.as_deref())?;
        let task = crate::harness::pi_proc::spawn_with_input_env(
            &argv,
            &req.repo_root,
            Some(req.prompt_body.clone()),
            &child_env,
        )?;
        stream::run(
            task,
            req,
            diagnostics,
            resource_bridge.as_ref(),
            planning_reads,
        )
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
