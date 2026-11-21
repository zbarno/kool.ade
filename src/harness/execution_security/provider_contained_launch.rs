//! Contained provider launch adapter (issue #591 phase 4, piece 2).
//!
//! Converts POSITIVE capability evidence (see
//! [`super::provider_eligibility`]) into a SEALED contained-launch plan
//! driven through the merged session port (see
//! [`crate::execution::docker::provider_session`]). The adapter is the
//! single choke point from "evidence says contained execution is ready"
//! to "the container engine receives a spec":
//!
//! - Planning refuses unless [`super::provider_eligibility`] evaluates the
//!   provider RUNNABLE: any missing/malformed evidence aborts BEFORE any
//!   engine interaction, carrying the full obstruction set for operators.
//! - Plans are built with the session builder (readonly rootfs, no-new-privs,
//!   all capabilities dropped, fixed pids/memory/cpu budgets) and are
//!   re-checked against the sealing constants before use.
//! - Container names extend the session ownership prefix
//!   (`koolade-provider-` + `{codex|claude}-{suffix}`) so the sealed
//!   teardown pass's ownership gate accepts them; the caller-supplied
//!   suffix carries per-request uniqueness.
//! - Network posture is still `Undecided` at the session layer until the
//!   phase-2a egress contract is frozen, so the plan CARRIES NO usable
//!   connectivity. Live provider execution end-to-end remains impossible on
//!   purpose: the default executor refuses and the network enum has no open
//!   variant. Fixture-executor tests prove sequencing, no-engine-touch on
//!   refusal, and verified teardown without claiming to run providers.
//!
//! Teardown uses the session's verified-absent protocol
//! ([`crate::execution::docker::provider_session::terminate_then_cleanup`]):
//! success means *verified gone*; residue is reported, never absorbed.
//!
//! Wired (consumer landed): the worker-launch integration
//! (`crate::app::root::harness_selection::wrappers::contained_worker`) drives
//! this adapter for admitted Worker-role codex/claude routes; tests keep
//! exercising the face directly.

use crate::execution::docker::provider_session::{
    CapturedExecReport, ProviderContainerSpec, ProviderExecutorPort, ProviderSpecBuilder,
    SESSION_MAX_CONTAINER_NAME_LEN, SESSION_NAME_PREFIX, terminate_then_cleanup,
};

use super::CliProvider;
use super::provider_eligibility::{
    ContainmentObstruction, ProviderContainmentEvidence, TaskExecutionVerdict,
    evaluate_task_execution,
};

/// A proposed contained run: the sealed spec plus the in-container command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainedLaunchPlan {
    /// Fully sealed spec handed to the executor.
    pub spec: ProviderContainerSpec,
    /// Arguments executed inside the container (resolved provider command).
    pub argv: Vec<String>,
}

/// Why a contained launch was refused. Variants split PRE-ENGINE planning
/// failures from MID-RUN engine failures (which always carry the teardown
/// outcome of the adapter's own recovery pass).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainedLaunchError {
    /// The provider is not governed by the managed-hook evaluator.
    UnsupportedProvider { provider: String },
    /// Capability evidence did not evaluate to runnable; carries the FULL
    /// outstanding obstruction list in stable order.
    InsufficientCapability {
        obstructions: Vec<ContainmentObstruction>,
    },
    /// Spec assembly violated the sealing invariants.
    SpecRejected { detail: String },
    /// An engine interaction failed; `teardown_ok` reflects whether the
    /// adapter's verified-absent recovery pass succeeded.
    Engine { detail: String, teardown_ok: bool },
}

/// Scratch grant (bytes) for provider working data; fixed by the slice, not
/// caller-tunable, so containment budgets stay singular.
pub const SCRATCH_TMPFS_BYTES: u64 = 256 * 1024 * 1024;

fn provider_slug(provider: &CliProvider) -> &'static str {
    match provider {
        CliProvider::Codex => "codex",
        CliProvider::Claude => "claude",
        _ => "",
    }
}

/// Build the container identity. The suffix must be caller-unique per
/// concurrent execution (request id / uuid fragment); it is lowercased
/// alphanumeric+dashes so the name stays a stable identifier.
fn container_name(provider: CliProvider, suffix: &str) -> String {
    // Budget the suffix against the session's name cap EXACTLY: prefix +
    // slug + separator already consume part of it, and slugs differ per
    // provider (codex=5, claude=6), so the safe suffix length differs too.
    let reserved = SESSION_NAME_PREFIX.len() + provider_slug(&provider).len() + 1;
    let max_suffix = SESSION_MAX_CONTAINER_NAME_LEN.saturating_sub(reserved);
    let cleaned: String = suffix
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(max_suffix.max(4))
        .collect();
    let suffix = if cleaned.is_empty() {
        "req".to_string()
    } else {
        cleaned
    };
    format!("{SESSION_NAME_PREFIX}{}-{suffix}", provider_slug(&provider))
}

/// Build a contained launch plan PURELY from positive evidence. Never
/// touches the engine.
pub fn plan_contained_launch(
    provider: CliProvider,
    evidence: &ProviderContainmentEvidence,
    request_suffix: &str,
    argv: Vec<String>,
) -> Result<ContainedLaunchPlan, ContainedLaunchError> {
    match provider {
        CliProvider::Codex | CliProvider::Claude => {}
        other => {
            return Err(ContainedLaunchError::UnsupportedProvider {
                provider: format!("{other:?}"),
            });
        }
    }
    match evaluate_task_execution(provider, evidence) {
        TaskExecutionVerdict::Runnable => {}
        TaskExecutionVerdict::Obstructed(obstructions) => {
            return Err(ContainedLaunchError::InsufficientCapability { obstructions });
        }
        TaskExecutionVerdict::OutOfScope => {
            return Err(ContainedLaunchError::UnsupportedProvider {
                provider: format!("{provider:?}"),
            });
        }
    }
    // A Runnable verdict guarantees a present, grammatical pin (enforced in
    // the evaluator); unwrap-shaped access is avoided by construction here.
    let Some(pin) = evidence.image_pin.as_ref() else {
        return Err(ContainedLaunchError::SpecRejected {
            detail: "runnable verdict without a pin (impossible state; fail closed)".into(),
        });
    };
    let builder = ProviderSpecBuilder::new(&pin.reference, &pin.digest)
        .container_name(container_name(provider, request_suffix))
        .add_scratch_tmpfs("/tmp", SCRATCH_TMPFS_BYTES)
        .map_err(|err| ContainedLaunchError::SpecRejected {
            detail: format!("{err}"),
        })?;
    let spec = builder
        .build()
        .map_err(|err| ContainedLaunchError::SpecRejected {
            detail: format!("{err}"),
        })?;
    if !spec.is_sealed() {
        return Err(ContainedLaunchError::SpecRejected {
            detail: "built spec failed the sealing invariants (budgets/caps/rootfs)".to_string(),
        });
    }
    Ok(ContainedLaunchPlan { spec, argv })
}

/// Recover through the session's verified-absent pass; true only when the
/// container is verified GONE.
fn run_teardown_pass<E: ProviderExecutorPort>(executor: &mut E, name: &str) -> bool {
    match terminate_then_cleanup(executor as &mut dyn ProviderExecutorPort, name) {
        Ok(record) => record.ok(),
        Err(_ownership_or_engine) => false,
    }
}

/// Drive a plan through an executor. Any failure mid-sequencing initiates
/// the verified teardown pass before returning; even success ends with the
/// pass so the adapter leaves NOTHING attributed to it.
pub fn run_planned_contained<E: ProviderExecutorPort>(
    executor: &mut E,
    plan: &ContainedLaunchPlan,
) -> Result<CapturedExecReport, ContainedLaunchError> {
    let name = plan.spec.container_name.clone();
    let fail = |what: &str, err_display: String, executor: &mut E| ContainedLaunchError::Engine {
        detail: format!("{what}: {err_display}"),
        teardown_ok: run_teardown_pass(executor, &name),
    };
    if let Err(err) = executor.create(&plan.spec) {
        return Err(fail("create", format!("{err}"), executor));
    }
    match executor.exec(&name, &plan.argv) {
        Ok(report) => {
            let teardown_ok = run_teardown_pass(executor, &name);
            if !teardown_ok {
                return Err(ContainedLaunchError::Engine {
                    detail: format!(
                        "exec finished ({}) but teardown could not verify absence",
                        report.exit_code
                    ),
                    teardown_ok: false,
                });
            }
            Ok(report)
        }
        Err(err) => Err(fail("exec", format!("{err}"), executor)),
    }
}

#[cfg(test)]
#[path = "provider_contained_launch/tests.rs"]
mod tests;
