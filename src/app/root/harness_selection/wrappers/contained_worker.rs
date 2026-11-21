//! Contained worker harness: routed Worker-role Codex/Claude work executes
//! inside its SEALED provider namespace instead of on the host (#591
//! phase 4, wiring slice A).
//!
//! Admission was decided upstream at selection time — this wrapper only
//! exists on routes that already passed the containment evaluator's
//! `Runnable` verdict (seam: `routing::routed_harness`). The execute path
//! nevertheless RE-CHECKS admission at call time: a contained launch is a
//! security act and never trusts selection-time state blindly.
//!
//! Deliberate deferrals (tracked as follow-on slices): host-side
//! credential environment into the image (containers get no ambient env;
//! the promoted image must carry what the CLI needs), REQUESTED-MODEL
//! FORWARDING (v1 runs the sealed image's built-in CLI defaults; the
//! one-shot argv deliberately carries no model selector until the
//! model-selection contract ships with the image-promotion slice —
//! characterized by test), interactive/ACP
//! stream mediation (v1 is one-shot terminal-result execution), and
//! mid-run cancellation (this bollard vintage has no standalone exec
//! kill; the cooperative flag is honored at SUBMISSION only).
use crate::{
    app::root::harness_selection::wrappers::contained_lane::{self, ContainedRunFault},
    error::AppError,
    execution::docker::provider_session::{
        CapturedExecReport, ProviderContainerSpec, ProviderExecutorPort, ProviderSessionError,
    },
    harness::{
        AiHarness, ExecutionProfile, HarnessOutcome, LiveProgress, PlanningRequest, RetrievalPlan,
        execution_security::{
            CliProvider, ContainedLaunchError, ContainedLaunchPlan, ProviderContainmentEvidence,
            plan_contained_launch, profile_block_reason, provider_evidence_snapshot,
            run_planned_contained,
        },
    },
};
use std::sync::{Arc, atomic::Ordering};

/// Builds the executor that talks to the engine. Abstracted so tests can
/// substitute a recording fake; production always uses the real driver.
/// Driving a contained run is an APP decision, not an
/// execution-infrastructure one, hence the abstraction lives here.
pub(crate) trait ContainedExecutorFactory: Send + Sync {
    fn create_executor(
        &self,
    ) -> Result<
        Box<dyn crate::execution::docker::provider_session::ProviderExecutorPort + Send>,
        String,
    >;
}

pub(super) struct ProductionContainedExecutorFactory;

impl ContainedExecutorFactory for ProductionContainedExecutorFactory {
    fn create_executor(
        &self,
    ) -> Result<
        Box<dyn crate::execution::docker::provider_session::ProviderExecutorPort + Send>,
        String,
    > {
        Ok(Box::new(
            crate::execution::docker::provider_driver::ProviderContainerDriver::connect()
                .map_err(|error| error.to_string())?,
        ))
    }
}

/// Sized carrier so the sequencer's generic executor parameter (generics
/// imply `Sized` by default) can hold an object-safe handle.
struct DynExecutorShim(Box<dyn ProviderExecutorPort + Send>);

impl ProviderExecutorPort for DynExecutorShim {
    fn create(&mut self, spec: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
        self.0.create(spec)
    }
    fn exec(
        &mut self,
        name: &str,
        argv: &[String],
    ) -> Result<CapturedExecReport, ProviderSessionError> {
        self.0.exec(name, argv)
    }
    fn terminate(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        self.0.terminate(name)
    }
    fn verify_absent(&mut self, name: &str) -> bool {
        self.0.verify_absent(name)
    }
    fn remove(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        self.0.remove(name)
    }
}

/// Admission gate bound to the live administrative policy in production;
/// injectable for hermetic tests (see `with_parts`).
pub(crate) type ContainedPolicyGate =
    Arc<dyn Fn(&str, ExecutionProfile) -> Option<String> + Send + Sync>;

/// Process evidence supply; production = the admission snapshot, tests =
/// fabricated admitted evidence.
pub(crate) type ContainedEvidenceSource =
    Arc<dyn Fn() -> Arc<ProviderContainmentEvidence> + Send + Sync>;

pub(crate) struct ContainedWorkerHarness {
    /// The legacy provider harness, kept strictly for READ-ONLY behaviors
    /// (label, availability diagnostics, provenance). Its process-spawn
    /// path is never exercised through this wrapper.
    inner: Box<dyn AiHarness>,
    harness_id: String,
    factory: Arc<dyn ContainedExecutorFactory>,
    /// Production: the live administrative policy gate (`profile_block_
    /// reason`). Injected so tests can isolate the evidence/planner axis
    /// from the process-level hook policy.
    policy_gate: ContainedPolicyGate,
    /// Production: the process-level admission snapshot. Tests substitute
    /// fabricated admitted evidence through the same seam as the factory.
    evidence_source: ContainedEvidenceSource,
}

impl ContainedWorkerHarness {
    pub(crate) fn new(inner: Box<dyn AiHarness>, harness_id: String) -> Self {
        Self {
            inner,
            harness_id,
            factory: Arc::new(ProductionContainedExecutorFactory),
            policy_gate: Arc::new(|id: &str, profile: ExecutionProfile| {
                profile_block_reason(id, profile).map(str::to_string)
            }),
            evidence_source: Arc::new(provider_evidence_snapshot),
        }
    }

    /// Test seam: substitute the executor factory AND the evidence
    /// source (fabricated admission, no engine, no env switches).
    #[cfg(test)]
    pub(crate) fn with_parts(
        inner: Box<dyn AiHarness>,
        harness_id: String,
        factory: Arc<dyn ContainedExecutorFactory>,
        policy_gate: ContainedPolicyGate,
        evidence_source: ContainedEvidenceSource,
    ) -> Self {
        Self {
            inner,
            harness_id,
            factory,
            policy_gate,
            evidence_source,
        }
    }

    /// The provider this wrapper encloses. Construction happens only at
    /// the routed seam for codex/claude, so this cannot realistically
    /// miss; fail loud if it ever does.
    fn provider(&self) -> CliProvider {
        CliProvider::from_id(&self.harness_id).unwrap_or_else(|| {
            panic!(
                "contained wrapper constructed for unroutable provider id {:?}",
                self.harness_id
            )
        })
    }

    /// v0 image-side CLI contract: one-shot non-interactive invocation at
    /// the promoted-image layout path. This is the ONE place the
    /// image-promotion slice must keep in sync with the actual staged
    /// layout; change it here, never scatter it at call sites.
    fn contained_one_shot_argv(provider: CliProvider, prompt: &str) -> Option<Vec<String>> {
        if prompt.trim().is_empty() {
            return None;
        }
        let (binary, flag): (&str, &[&str]) = match provider {
            CliProvider::Codex => ("/usr/local/bin/codex", &["exec"]),
            CliProvider::Claude => ("/usr/local/bin/claude", &["-p"]),
            _ => return None,
        };
        Some(
            vec![binary.to_string()]
                .into_iter()
                .chain(flag.iter().map(|part| (*part).to_string()))
                .chain([prompt.to_string()])
                .collect(),
        )
    }

    /// Gates + translation, PURE with respect to the engine. Produces the
    /// ready-to-drive plan.
    fn prepare(&self, request: &PlanningRequest) -> Result<ContainedLaunchPlan, AppError> {
        if request.cancel.load(Ordering::Acquire) {
            return Err(AppError::Other(
                "Cancelled before the contained run was offered to its execution lane".into(),
            ));
        }
        // Call-time admission re-check. The process-level evidence
        // snapshot is stable, so this primarily DOCUMENTS the invariant
        // (execution acts only on routes that are Runnable NOW) and fails
        // loudly if selection and execution ever diverge.
        if let Some(reason) = (self.policy_gate)(&self.harness_id, ExecutionProfile::Worker) {
            return Err(AppError::Other(format!(
                "Contained worker execution blocked: {reason}"
            )));
        }
        let provider = self.provider();
        let prompt = request.prompt_body.trim();
        let argv = Self::contained_one_shot_argv(provider, prompt).ok_or_else(|| {
            AppError::Other(format!(
                "Provider {} has no one-shot contained invocation convention \
                 (or the prompt body was empty)",
                self.harness_id
            ))
        })?;
        let suffix = uuid::Uuid::new_v4()
            .hyphenated()
            .to_string()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect::<String>();
        let evidence = (self.evidence_source)();
        plan_contained_launch(provider, &evidence, &suffix, argv).map_err(planning_error_to_app)
    }

    /// Hands the plan to the containment lane and translates the outcome.
    fn drive(
        &self,
        plan: &ContainedLaunchPlan,
        notify: impl Fn(&str),
    ) -> Result<CapturedExecReport, AppError> {
        let factory = Arc::clone(&self.factory);
        let plan_clone = plan.clone();
        notify("Launching contained provider namespace…");
        let outcome = contained_lane::submit(Box::new(move || {
            let executor = factory
                .create_executor()
                .map_err(ContainedRunFault::Connect)?;
            let mut shim = DynExecutorShim(executor);
            run_planned_contained(&mut shim, &plan_clone).map_err(ContainedRunFault::Launch)
        }));
        match outcome {
            Err(fault) => {
                notify("Contained run failed in the containment lane.");
                Err(AppError::Io {
                    op: "contained-exec".into(),
                    detail: fault.describe(),
                })
            }
            Ok(report) => {
                notify("Contained run finished; assembling result.");
                Ok(report)
            }
        }
    }
}

impl AiHarness for ContainedWorkerHarness {
    fn label(&self) -> String {
        format!("{} (contained namespace)", self.inner.label())
    }

    fn check_available(&self) -> Result<String, AppError> {
        // Intentionally delegates: the UI's availability story describes
        // the HOST provider CLI (what operators authenticate and
        // discover). The contained run itself never invokes the host
        // binary; its path is gated by the admission evidence instead.
        self.inner.check_available()
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let plan = self.prepare(request)?;
        let progress_tx = request.progress_tx.clone();
        let notify = |message: &str| {
            let snapshot = LiveProgress {
                response: message.to_string(),
                ..LiveProgress::default()
            };
            let _ = progress_tx.send(snapshot);
        };
        let report = self.drive(&plan, notify)?;
        if report.exit_code == 0 {
            Ok(HarnessOutcome {
                final_text: report.stdout_tail,
                envelope: None,
                stderr_tail: report.stderr_tail,
            })
        } else {
            Err(AppError::HarnessFailed {
                reason: format!(
                    "provider CLI exited {} inside its contained namespace",
                    report.exit_code
                ),
                stderr_tail: report.stderr_tail,
            })
        }
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        // Workers keep the legacy retrieval stance (usually none); the
        // contained path adds no retrieval. Defense in depth: a WRAPPED
        // provider gaining a real host-spawn retrieval implementation
        // later would silently reopen the host-seam through this delegate
        // — revisit this line whenever retrieval support lands in a
        // wrapped provider.
        self.inner.plan_retrieval(request)
    }

    fn route_provenance(
        &self,
        request: &PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        self.inner.route_provenance(request)
    }
}

fn planning_error_to_app(err: ContainedLaunchError) -> AppError {
    match err {
        ContainedLaunchError::UnsupportedProvider { provider } => AppError::Other(format!(
            "Provider {provider} is not routable through containment"
        )),
        ContainedLaunchError::InsufficientCapability { obstructions } => AppError::Other(format!(
            "Containment admission is no longer sufficient at execution time: {obstructions:?}"
        )),
        ContainedLaunchError::SpecRejected { detail } => AppError::Io {
            op: "contained-plan".into(),
            detail,
        },
        ContainedLaunchError::Engine {
            detail,
            teardown_ok,
        } => AppError::Io {
            op: "contained-plan".into(),
            detail: format!(
                "planning-stage engine report: {detail} (teardown verified: {})",
                if teardown_ok { "yes" } else { "no" }
            ),
        },
    }
}

#[cfg(test)]
#[path = "contained_worker/tests.rs"]
mod tests;
