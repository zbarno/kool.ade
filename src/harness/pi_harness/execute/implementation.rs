use super::super::{PiHarness, diagnostics};
use crate::error::AppError;
use crate::harness::pi_extract::extract_json_object;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest, RetrievalPlan, ToolAccess};

mod clean_profile;
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
        crate::harness::validate_reasoning_level("pi", &req.reasoning_level)?;
        crate::harness::require_application_implementation_boundary("Pi", req)?;
        let boundary =
            crate::harness::execution_security::ApplicationBoundary::new(req).map_err(|error| {
                AppError::Other(format!(
                    "Cannot start Kool.ad/e execution session: {error:#}"
                ))
            })?;
        let exe = Self::locate_binary()?;
        let exe = exe
            .canonicalize()
            .map_err(|error| AppError::Other(format!("Cannot resolve Pi executable: {error}")))?;
        let version = PiHarness::check_binary(&exe)?;
        let planning_reads = boundary.access() == ToolAccess::ReadOnly;
        let profile =
            clean_profile::prepare(boundary.working_directory(), model).map_err(|error| {
                AppError::Other(format!(
                    "Cannot prepare isolated Pi configuration: {error:#}"
                ))
            })?;
        let mut argv = vec![exe.to_string_lossy().into_owned()];
        argv.extend([
            "--provider".into(),
            profile.provider.clone(),
            "--model".into(),
            profile.model.clone(),
        ]);
        let system_instructions = system_instructions(req, boundary.system_policy());
        argv.extend([
            "--print".into(),
            "--mode".into(),
            "json".into(),
            "--no-session".into(),
            "--no-approve".into(),
            "--no-context-files".into(),
            "--no-extensions".into(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--no-themes".into(),
            "--append-system-prompt".into(),
            format!("PROCESS OWNERSHIP — mandatory for every tool call: Kool.ad/e is the supervising application (PID {}). Never signal or terminate Kool.ad/e, its ancestors, other operator windows, or unrelated workers. Do not use pkill/killall, command-name or command-line matching, or machine-wide process sweeps to select kill targets. Test cleanup may stop only processes you launched and recorded for that test run. For detached GUI children, require a unique inherited run marker plus the exact executable and test display; verify ownership before each signal and use pidfds where available to avoid PID reuse. Inspect existing cleanup helpers before running them; repair broad process matching first. If ownership cannot be established, preserve the process and report it. A private test display alone does not isolate processes or authorize killing other app instances.\n\n{}", std::process::id(), system_instructions),
            "--thinking".into(),
            req.reasoning_level.clone(),
        ]);

        let mut child_env = vec![
            (
                "PI_CODING_AGENT_DIR".into(),
                profile.agent_directory.to_string_lossy().into_owned(),
            ),
            (
                "PI_CODING_AGENT_SESSION_DIR".into(),
                profile.session_directory.to_string_lossy().into_owned(),
            ),
            (
                "PI_PACKAGE_DIR".into(),
                profile.package_directory.to_string_lossy().into_owned(),
            ),
            ("PI_OFFLINE".into(), "1".into()),
            ("PI_SKIP_VERSION_CHECK".into(), "1".into()),
        ];
        boundary
            .configure(
                crate::harness::execution_security::CliProvider::Pi,
                &mut argv,
                &mut child_env,
            )
            .map_err(|error| {
                AppError::Other(format!("Cannot configure Pi application tools: {error:#}"))
            })?;
        let diagnostics = diagnostics::open(boundary.working_directory(), req.mode)?;
        let provider = crate::harness::execution_security::CliProvider::Pi;
        let evidence = crate::harness::execution_security::ProfileProbeEvidence::probe(
            provider, &exe, version,
        )
        .map_err(AppError::from_provider_profile_error)?;
        let launch = crate::harness::execution_security::ValidatedProviderLaunch::bind(
            evidence,
            argv,
            boundary.working_directory(),
            child_env,
        )
        .map_err(|error| AppError::Other(format!("Pi launch profile is invalid: {error}")))?;
        let launch_profile = launch.telemetry(
            req.prompt_manifest
                .as_ref()
                .map(|manifest| manifest.role_version.as_str()),
        );
        let task = launch.spawn_with_input(boundary.working_directory(), Some(user_input(req)))?;
        stream::run(
            task,
            req,
            diagnostics,
            boundary.resource_bridge(),
            planning_reads,
            launch_profile,
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

pub(in crate::harness) fn system_instructions(
    req: &PlanningRequest,
    boundary_policy: &str,
) -> String {
    format!("{}\n\n{}", req.system_instructions, boundary_policy)
}

pub(in crate::harness) fn user_input(req: &PlanningRequest) -> String {
    req.prompt_body.clone()
}
