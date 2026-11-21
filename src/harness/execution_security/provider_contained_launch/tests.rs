//! Proof tests for the contained launch adapter (issue #591 piece 2).
//!
//! Provenances exercised WITHOUT claiming real execution: plan sealing,
//! name ownership, refusal-before-engine, fixture sequencing, and the
//! default executor's inert refusal path.

use super::*;
use crate::execution::docker::provider_session::{
    ProviderNetworkMode, ProviderSessionError, TerminationRecord, terminate_then_cleanup,
};

fn full_evidence() -> ProviderContainmentEvidence {
    ProviderContainmentEvidence {
        platform_os: crate::harness::execution_security::provider_eligibility::PlatformFact::Linux,
        engine_verified: true,
        image_pin: Some(
            crate::harness::execution_security::provider_eligibility::ContainedImagePin {
                reference: "ghcr.io/zbarno/koolade-provider-cli".to_string(),
                digest: format!("sha256:{}", "cc".repeat(32)),
            },
        ),
        hook_containment_attested: true,
    }
}

struct FixtureExecutor {
    alive: bool,
    created: usize,
    exec_calls: Vec<Vec<String>>,
    terminated: usize,
    removed: usize,
    create_outcome: Result<(), ()>,
    exec_outcome: Result<CapturedExecReport, ()>,
    /// When true, `verify_absent` lies "still present" even after terminate
    /// (models an engine that accepted the terminate but cannot confirm
    /// absence): the session must report RESIDUE and the adapter must
    /// surface it, never absorb it.
    lie_resident: bool,
}

impl FixtureExecutor {
    fn permissive() -> Self {
        Self {
            alive: false,
            created: 0,
            exec_calls: Vec::new(),
            terminated: 0,
            removed: 0,
            create_outcome: Ok(()),
            exec_outcome: Ok(CapturedExecReport::bare(0)),
            lie_resident: false,
        }
    }
    fn lie_resident(mut self) -> Self {
        self.lie_resident = true;
        self
    }
    fn fail_create(mut self) -> Self {
        self.create_outcome = Err(());
        self
    }
    fn exec_code(self, code: i32) -> Self {
        Self {
            exec_outcome: Ok(CapturedExecReport::bare(code)),
            ..self
        }
    }
    fn fail_exec(mut self) -> Self {
        self.exec_outcome = Err(());
        self
    }
    fn error(&self) -> ProviderSessionError {
        // Stand-in engine failure identity; only the SEQUENCING is under
        // test, not the engine's own diagnostic.
        ProviderSessionError::ExecBeforeStart
    }
}

impl ProviderExecutorPort for FixtureExecutor {
    fn create(&mut self, _spec: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
        self.created += 1;
        let outcome = std::mem::replace(&mut self.create_outcome, Ok(()));
        outcome.map_err(|_| self.error())?;
        self.alive = true;
        Ok(())
    }
    fn exec(
        &mut self,
        _name: &str,
        argv: &[String],
    ) -> Result<CapturedExecReport, ProviderSessionError> {
        if !self.alive {
            return Err(self.error());
        }
        self.exec_calls.push(argv.to_vec());
        self.exec_outcome.clone().map_err(|_| self.error())
    }
    fn terminate(&mut self, _name: &str) -> Result<(), ProviderSessionError> {
        self.terminated += 1;
        if !self.alive {
            return Err(self.error());
        }
        self.alive = false;
        Ok(())
    }
    fn verify_absent(&mut self, _name: &str) -> bool {
        !self.alive && !self.lie_resident
    }
    fn remove(&mut self, _name: &str) -> Result<(), ProviderSessionError> {
        self.removed += 1;
        self.alive = false;
        Ok(())
    }
}

#[test]
fn complete_evidence_yields_a_sealed_well_named_plan() {
    for (provider, slug) in [
        (CliProvider::Codex, "codex"),
        (CliProvider::Claude, "claude"),
    ] {
        let plan = plan_contained_launch(
            provider,
            &full_evidence(),
            "req42",
            vec![String::from("run")],
        )
        .unwrap_or_else(|err| panic!("{provider:?}: {err:?}"));
        assert!(plan.spec.is_sealed(), "{provider:?}: spec must seal");
        assert_eq!(
            plan.spec.container_name,
            format!("koolade-provider-{slug}-req42")
        );
        assert_eq!(
            plan.spec.network,
            ProviderNetworkMode::Undecided,
            "no connectivity until the egress contract is frozen"
        );
        assert_eq!(plan.spec.mounts.len(), 1, "only the /tmp scratch grant");
        assert_eq!(
            plan.spec.image_reference,
            "ghcr.io/zbarno/koolade-provider-cli"
        );
        assert_eq!(
            plan.spec.image_digest,
            format!("sha256:{}", "cc".repeat(32))
        );
        assert_eq!(plan.argv, vec!["run"]);
    }
}

#[test]
fn name_suffix_is_sanitized_and_bounded() {
    let plan = plan_contained_launch(
        CliProvider::Codex,
        &full_evidence(),
        "AB_9 z!!xy-1zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        vec![],
    )
    .unwrap();
    // Filters keep [a-z0-9-]; expected: digits/lowercase/dash retained.
    let expected_suffix: String = "AB_9 z!!xy-1zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .take(40)
        .collect();
    assert_eq!(
        plan.spec.container_name,
        format!("koolade-provider-codex-{expected_suffix}")
    );
    // Deterministic: same input, same name.
    let again = plan_contained_launch(
        CliProvider::Codex,
        &full_evidence(),
        "AB_9 z!!xy-1zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        vec![],
    )
    .unwrap();
    assert_eq!(again.spec.container_name, plan.spec.container_name);
    // Degenerate suffix falls back rather than colliding to a bare name.
    let fallback =
        plan_contained_launch(CliProvider::Codex, &full_evidence(), "***", vec![]).unwrap();
    assert_eq!(fallback.spec.container_name, "koolade-provider-codex-req");
}

#[test]
fn non_managed_hook_providers_have_no_adapter_at_all() {
    for provider in [
        CliProvider::Pi,
        CliProvider::OpenCode,
        CliProvider::Copilot,
        CliProvider::Antigravity,
    ] {
        match plan_contained_launch(provider, &full_evidence(), "x", vec![]) {
            Err(ContainedLaunchError::UnsupportedProvider { provider: p }) => {
                assert!(!p.is_empty());
            }
            other => panic!("{provider:?}: expected unsupported, got {other:?}"),
        }
    }
}

#[test]
fn every_evidence_hole_aborts_before_any_engine_surface() {
    // Structurally plan() takes no executor handle: refusal cannot mutate
    // anything. Assert the payload carries the full recovery list.
    match plan_contained_launch(
        CliProvider::Codex,
        &ProviderContainmentEvidence::default(),
        "x",
        vec![],
    ) {
        Err(ContainedLaunchError::InsufficientCapability { obstructions }) => {
            assert_eq!(
                obstructions.len(),
                4,
                "default = all holes; got {:?}",
                obstructions
                    .iter()
                    .map(|o| o.describe())
                    .collect::<Vec<_>>()
            );
        }
        other => panic!("expected insufficient capability, got {other:?}"),
    }
    let mut ev = full_evidence();
    ev.engine_verified = false;
    match plan_contained_launch(CliProvider::Claude, &ev, "x", vec![]) {
        Err(ContainedLaunchError::InsufficientCapability { obstructions }) => {
            assert_eq!(obstructions.len(), 1);
            assert!(matches!(
                obstructions[0],
                ContainmentObstruction::EngineNotVerified
            ));
        }
        other => panic!("expected single engine hole, got {other:?}"),
    }
}

#[test]
fn the_ship_default_executor_stays_physically_refusing() {
    let plan = plan_contained_launch(
        CliProvider::Codex,
        &full_evidence(),
        "rt1",
        vec!["codex".into()],
    )
    .unwrap();
    let mut executor = crate::execution::docker::provider_session::RefusingUnpinnedExecutor;
    match run_planned_contained(&mut executor, &plan) {
        Err(ContainedLaunchError::Engine {
            detail,
            teardown_ok,
        }) => {
            assert!(detail.starts_with("create"), "{detail}");
            assert!(
                !teardown_ok,
                "absence cannot be verified against a refusing engine"
            );
        }
        other => panic!("expected engine refusal, got {other:?}"),
    }
}

#[test]
fn fixture_run_completes_with_verified_teardown_and_recorded_argv() {
    let plan = plan_contained_launch(
        CliProvider::Claude,
        &full_evidence(),
        "case7",
        vec![String::from("claude"), String::from("--task")],
    )
    .unwrap();
    let mut fx = FixtureExecutor::permissive();
    let report = run_planned_contained(&mut fx, &plan).expect("happy path completes");
    assert_eq!(report.exit_code, 0);
    assert_eq!(fx.created, 1);
    assert_eq!(
        fx.exec_calls,
        vec![vec![String::from("claude"), String::from("--task")]]
    );
    assert_eq!(fx.terminated, 1);
    assert_eq!(fx.removed, 1);
    assert!(!fx.alive, "nothing attributed to the adapter afterwards");
}

#[test]
fn nonzero_exit_propagates_after_successful_verified_teardown() {
    let plan = plan_contained_launch(CliProvider::Codex, &full_evidence(), "nz9", vec![]).unwrap();
    let mut fx = FixtureExecutor::permissive().exec_code(7);
    let report = run_planned_contained(&mut fx, &plan).expect("propagation");
    assert_eq!(report.exit_code, 7);
    assert!(fx.terminated >= 1 && fx.removed >= 1, "teardown ran");
}

#[test]
fn create_failure_reports_engine_failure_without_fake_success() {
    let plan = plan_contained_launch(CliProvider::Claude, &full_evidence(), "cf1", vec![]).unwrap();
    let mut fx = FixtureExecutor::permissive().fail_create();
    match run_planned_contained(&mut fx, &plan) {
        Err(ContainedLaunchError::Engine { detail, .. }) => {
            assert!(detail.starts_with("create"), "{detail}");
            assert_eq!(
                fx.exec_calls.len(),
                0,
                "exec must never precede a live create"
            );
        }
        other => panic!("expected create failure, got {other:?}"),
    }
}

#[test]
fn exec_failure_runs_the_recovery_pass_and_reports_it() {
    let plan = plan_contained_launch(CliProvider::Codex, &full_evidence(), "xf2", vec![]).unwrap();
    let mut fx = FixtureExecutor::permissive().fail_exec();
    match run_planned_contained(&mut fx, &plan) {
        Err(ContainedLaunchError::Engine {
            detail,
            teardown_ok,
        }) => {
            assert!(detail.starts_with("exec"), "{detail}");
            assert!(teardown_ok, "fixture teardown verifies absence");
        }
        other => panic!("expected exec failure, got {other:?}"),
    }
}

#[test]
fn teardown_gate_accepts_adapter_names_but_refuses_foreign_prefixes() {
    // The adapter's names sit under the session ownership prefix by
    // construction; foreign names are rejected at the ownership gate, so an
    // adapter-originated teardown pass can never sweep unrelated
    // containers.
    let mut fx = FixtureExecutor::permissive();
    let plan = plan_contained_launch(CliProvider::Codex, &full_evidence(), "og1", vec![]).unwrap();
    assert!(matches!(
        terminate_then_cleanup(
            &mut fx as &mut dyn ProviderExecutorPort,
            &plan.spec.container_name
        ),
        Ok(TerminationRecord {
            idempotent_already_absent: true,
            ..
        })
    ));
    assert!(matches!(
        terminate_then_cleanup(&mut fx as &mut dyn ProviderExecutorPort, "koolade-other-x"),
        Err(ProviderSessionError::OwnershipMismatch)
    ));
}

#[test]
fn unverified_presence_after_clean_exit_is_surfaced_not_absorbed() {
    // Strongest invariant of the slice: a CLEAN exec exit still ends in the
    // verified-absent gate; if absence cannot be confirmed, the adapter
    // reports Engine{teardown_ok:false} instead of returning success.
    let plan = plan_contained_launch(CliProvider::Codex, &full_evidence(), "ur1", vec![]).unwrap();
    let mut fx = FixtureExecutor::permissive().exec_code(0).lie_resident();
    match run_planned_contained(&mut fx, &plan) {
        Err(ContainedLaunchError::Engine {
            detail,
            teardown_ok,
        }) => {
            assert!(detail.starts_with("exec finished (0)"), "{detail}");
            assert!(!teardown_ok, "residue must not be absorbed as success");
        }
        Ok(report) => panic!(
            "absent-unverified clean exit leaked as Ok({})",
            report.exit_code
        ),
        other => panic!("expected Engine teardown report, got {other:?}"),
    }
}

#[test]
fn name_budget_accepts_full_length_suffixes_for_both_providers() {
    // The sanitized budget must fit BOTH slugs under the session name cap:
    // codex reserves less than claude, so a fixed truncation width would
    // silently reject full-length claude suffixes (spurious SpecRejected).
    let long_suffix: String = "abcdef0123456789xyz-abcdefghijklmnopqrstu"
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .collect();
    assert!(long_suffix.chars().count() >= 40, "test precondition");
    for (provider, slug) in [
        (CliProvider::Codex, "codex"),
        (CliProvider::Claude, "claude"),
    ] {
        let plan = plan_contained_launch(provider, &full_evidence(), &long_suffix, vec![])
            .unwrap_or_else(|err| panic!("{slug}: {err:?}"));
        assert!(
            plan.spec.container_name.len() <= SESSION_MAX_CONTAINER_NAME_LEN,
            "{slug}: name must respect the session cap: {}",
            plan.spec.container_name
        );
        assert!(
            plan.spec
                .container_name
                .starts_with(&format!("{SESSION_NAME_PREFIX}{slug}-"))
        );
    }
    // Boundary: a wide 48-char sanitizable suffix stays admissible for the
    // longer slug too (budget derives from measurement, not a constant).
    let bigger = "abcdefghij0123456789abcdefghij0123456789xy";
    let plan =
        plan_contained_launch(CliProvider::Claude, &full_evidence(), bigger, vec![]).unwrap();
    assert!(plan.spec.container_name.len() <= SESSION_MAX_CONTAINER_NAME_LEN);
}
