use super::{ContainedExecutorFactory, ContainedWorkerHarness};
use crate::app::root::harness_selection::wrappers::contained_lane::test_support;
use crate::execution::docker::provider_session::{
    CapturedExecReport, ProviderContainerSpec, ProviderExecutorPort, ProviderSessionError,
};
use crate::harness::execution_security::{
    CliProvider, ContainedImagePin, ProviderContainmentEvidence, detect_platform_fact,
};
use crate::harness::{
    AiHarness, ExecutionMode, ExecutionProfile, HarnessOutcome, LiveProgress, PlanningRequest,
};
use std::sync::{Arc, Mutex, mpsc};

/// Records every port call; exec returns the preset report.
struct FakePort {
    preset: CapturedExecReport,
    record: Arc<Mutex<FakeRecord>>,
}

#[derive(Default)]
struct FakeRecord {
    created_names: Vec<String>,
    exec_calls: Vec<(String, Vec<String>)>,
    terminated: Vec<String>,
    removed: Vec<String>,
}

impl ProviderExecutorPort for FakePort {
    fn create(&mut self, spec: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
        self.record
            .lock()
            .unwrap()
            .created_names
            .push(spec.container_name.clone());
        Ok(())
    }

    fn exec(
        &mut self,
        name: &str,
        argv: &[String],
    ) -> Result<CapturedExecReport, ProviderSessionError> {
        self.record
            .lock()
            .unwrap()
            .exec_calls
            .push((name.to_string(), argv.to_vec()));
        Ok(self.preset.clone())
    }

    fn terminate(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        self.record
            .lock()
            .unwrap()
            .terminated
            .push(name.to_string());
        Ok(())
    }

    fn verify_absent(&mut self, _name: &str) -> bool {
        true
    }

    fn remove(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        self.record
            .lock()
            .unwrap()
            .removed
            .push(format!("remove:{name}"));
        Ok(())
    }
}

struct FakeFactory {
    preset: CapturedExecReport,
    record: Arc<Mutex<FakeRecord>>,
}

impl ContainedExecutorFactory for FakeFactory {
    fn create_executor(&self) -> Result<Box<dyn ProviderExecutorPort + Send>, String> {
        Ok(Box::new(FakePort {
            preset: self.preset.clone(),
            record: Arc::clone(&self.record),
        }))
    }
}

fn preset_report(exit_code: i32, stdout: &str, stderr: &str) -> CapturedExecReport {
    CapturedExecReport {
        exit_code,
        stdout_tail: stdout.to_string(),
        stderr_tail: stderr.to_string(),
        stdout_bytes: stdout.len() as u64,
        stderr_bytes: stderr.len() as u64,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn admitted_evidence() -> Arc<ProviderContainmentEvidence> {
    // Fabricated POSITIVE side of the ladder: verified engine,
    // digest-pinned image, attested hooks, host platform. Reaches this
    // state in production only after the ops/promotion slices complete.
    Arc::new(ProviderContainmentEvidence {
        platform_os: detect_platform_fact(),
        engine_verified: true,
        image_pin: Some(ContainedImagePin {
            reference: "ghcr.io/koolade-testing/provider-cli-pin".to_string(),
            digest: format!("sha256:{:064x}", 1234_u64),
        }),
        hook_containment_attested: true,
    })
}

fn dummy_request(prompt: &str, cancelled: bool) -> (PlanningRequest, mpsc::Receiver<LiveProgress>) {
    let (progress_tx, progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some("task-1".to_string()),
        execution_workspace: None,
        investigation_authorization: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: None,
        reasoning_level: "low".to_string(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/tmp"),
        runtime_config_source: None,
        prompt_body: prompt.to_string(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(60),
        progress_tx,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(cancelled)),
        review_identity: None,
        computer_use: None,
    };
    (request, progress_rx)
}

/// Read-only inner stand-in: supplies exactly what the wrapper may lawfully
/// read, and PANICS if anyone tries to drive its spawn path.
struct InnerStub;

impl AiHarness for InnerStub {
    fn label(&self) -> String {
        "stub-cli 0.0.0-test".to_string()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("0.0.0-test".to_string())
    }
    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        panic!("legacy spawn path must never run through the contained wrapper")
    }
}

fn open_policy(_harness_id: &str, _profile: crate::harness::ExecutionProfile) -> Option<String> {
    // Test-only stand-in: isolates the evidence/planner axes from the
    // process-level administrative hook policy (exercised separately by
    // the production-wiring block test through `new()`).
    None
}

fn admitted_harness(factory: Arc<FakeFactory>) -> ContainedWorkerHarness {
    ContainedWorkerHarness::with_parts(
        Box::new(InnerStub),
        "codex".into(),
        factory,
        Arc::new(open_policy),
        {
            let ev = admitted_evidence();
            Arc::new(move || Arc::clone(&ev))
        },
    )
}

#[test]
fn contained_argv_follows_the_documented_one_shot_conventions() {
    let argv = ContainedWorkerHarness::contained_one_shot_argv(CliProvider::Codex, "hello worker")
        .expect("codex has a convention");
    assert_eq!(argv, vec!["/usr/local/bin/codex", "exec", "hello worker"]);
    let argv = ContainedWorkerHarness::contained_one_shot_argv(CliProvider::Claude, "hello worker")
        .expect("claude has a convention");
    assert_eq!(argv, vec!["/usr/local/bin/claude", "-p", "hello worker"]);
    assert!(ContainedWorkerHarness::contained_one_shot_argv(CliProvider::Pi, "hello").is_none());
    assert!(ContainedWorkerHarness::contained_one_shot_argv(CliProvider::Codex, "   ").is_none());
}

#[test]
fn label_and_availability_decorate_without_spawning() {
    let harness = ContainedWorkerHarness::new(Box::new(InnerStub), "codex".into());
    assert_eq!(harness.label(), "stub-cli 0.0.0-test (contained namespace)");
    assert_eq!(harness.check_available().unwrap(), "0.0.0-test");
}

#[test]
fn execute_drives_the_full_laned_sequence_and_maps_success() {
    let record = Arc::new(Mutex::new(FakeRecord::default()));
    let factory = Arc::new(FakeFactory {
        preset: preset_report(0, "FINAL ANSWER\n", ""),
        record: Arc::clone(&record),
    });
    let harness = ContainedWorkerHarness::with_parts(
        Box::new(InnerStub),
        "codex".into(),
        factory,
        Arc::new(open_policy),
        {
            let ev = admitted_evidence();
            Arc::new(move || Arc::clone(&ev))
        },
    );
    let _lane_guard = test_support::lane_consumer_exclusive();
    let (request, _rx) = dummy_request("do the thing", false);
    let outcome = harness.execute(&request).expect("run succeeds");
    assert_eq!(outcome.final_text, "FINAL ANSWER\n");
    assert!(outcome.envelope.is_none());
    let rec = record.lock().unwrap();
    assert_eq!(rec.created_names.len(), 1, "namespace created exactly once");
    assert!(
        rec.created_names[0].starts_with("koolade-provider-codex-"),
        "session naming contract holds: {}",
        rec.created_names[0]
    );
    assert_eq!(rec.exec_calls.len(), 1);
    assert_eq!(rec.exec_calls[0].0, rec.created_names[0]);
    assert_eq!(
        rec.exec_calls[0].1,
        vec!["/usr/local/bin/codex", "exec", "do the thing"]
    );
}

#[test]
fn execute_surfaces_nonzero_exits_as_harness_failed_with_stderr_tail() {
    let factory = Arc::new(FakeFactory {
        preset: preset_report(7, "", "boom inside namespace\n"),
        record: Arc::new(Mutex::new(FakeRecord::default())),
    });
    let harness = ContainedWorkerHarness::with_parts(
        Box::new(InnerStub),
        "claude".into(),
        factory,
        Arc::new(open_policy),
        {
            let ev = admitted_evidence();
            Arc::new(move || Arc::clone(&ev))
        },
    );
    let _lane_guard = test_support::lane_consumer_exclusive();
    let (request, _rx) = dummy_request("please", false);
    let err = harness.execute(&request).expect_err("nonzero exit");
    let crate::error::AppError::HarnessFailed {
        ref reason,
        ref stderr_tail,
    } = err
    else {
        panic!("expected HarnessFailed, got {err:?}");
    };
    assert!(reason.contains("exited 7"), "reason was: {reason}");
    assert_eq!(stderr_tail, "boom inside namespace\n");
}

#[test]
fn cancelled_requests_never_reach_the_lane() {
    let factory = Arc::new(FakeFactory {
        preset: preset_report(0, "x", ""),
        record: Arc::new(Mutex::new(FakeRecord::default())),
    });
    let harness = admitted_harness(factory);
    let (request, _rx) = dummy_request("late cancel", true);
    assert!(harness.execute(&request).is_err());
}

#[test]
fn empty_prompt_bodies_are_refused_before_any_engine_traffic() {
    let factory = Arc::new(FakeFactory {
        preset: preset_report(0, "x", ""),
        record: Arc::new(Mutex::new(FakeRecord::default())),
    });
    let harness = admitted_harness(factory);
    let (request, _rx) = dummy_request("   ", false);
    assert!(harness.execute(&request).is_err());
}

#[test]
fn production_wiring_stays_blocked_until_the_ops_ladder_completes() {
    // Default construction = production factories and the REAL process
    // evidence snapshot. PRECONDITION: passes on any host where the
    // ops ladder is incomplete — universally true today, in CI, and on
    // any build whose compiled-in image lock lacks a provider_cli pin
    // (env switches alone cannot admit; the pin gate precedes them).
    // Should the lock EVER ship a published pin with both attestation
    // switches exportable, this test flips and must be re-pointed at a
    // non-production evidence path.
    let harness = ContainedWorkerHarness::new(Box::new(InnerStub), "codex".into());
    let (request, _rx) = dummy_request("should not run", false);
    let err = harness.execute(&request).expect_err("must stay blocked");
    let message = format!("{err:?}");
    assert!(
        message.to_lowercase().contains("blocked")
            || message.to_lowercase().contains("admission")
            || message.to_lowercase().contains("containment")
            || message.to_lowercase().contains("obstruct"),
        "error should name the containment refusal: {message}"
    );
}

/// Axis-independence proof: the CALL-TIME POLICY gate (admin-managed
/// hook containment) is consulted independently of the admission
/// evidence ladder — even SYNTHETICALLY-FULLY-ADMITTED evidence must not
/// outrank a live block.
#[test]
fn closed_live_policy_blocks_even_with_fully_admitted_evidence() {
    fn closed_policy_stub(_: &str, _: ExecutionProfile) -> Option<String> {
        Some("admin-managed hooks cannot be contained".to_string())
    }
    let record = Arc::new(Mutex::new(FakeRecord::default()));
    let harness = ContainedWorkerHarness::with_parts(
        Box::new(InnerStub),
        "codex".into(),
        Arc::new(FakeFactory {
            preset: preset_report(0, "ok-model", ""),
            record: record.clone(),
        }),
        Arc::new(closed_policy_stub),
        Arc::new({
            let ev = admitted_evidence();
            move || Arc::clone(&ev)
        }),
    );
    let (request, _rx) = dummy_request("admitted evidence, closed policy", false);
    let err = harness
        .execute(&request)
        .expect_err("the live policy gate outranks admitted evidence");
    assert!(
        format!("{err:?}").contains("admin-managed hooks cannot be contained"),
        "refusal should quote the live policy: {err:?}"
    );
    assert!(
        record.lock().unwrap().created_names.is_empty(),
        "a live block must not allocate any provider namespace"
    );
}

/// CHARACTERIZATION of the deliberate v1 model deferral: the requested
/// model selection is NOT forwarded into the in-container argv (the
/// sealed image's built-in CLI defaults govern); provenance still
/// reports the requested model, so this stability is asserted in one
/// place until the model-selection contract ships with the promotion
/// slice.
#[test]
fn execute_with_model_keeps_the_model_free_one_shot_argv_in_v1() {
    let record = Arc::new(Mutex::new(FakeRecord::default()));
    let harness = ContainedWorkerHarness::with_parts(
        Box::new(InnerStub),
        "codex".into(),
        Arc::new(FakeFactory {
            preset: preset_report(0, "ok-model", ""),
            record: record.clone(),
        }),
        Arc::new(open_policy),
        Arc::new({
            let ev = admitted_evidence();
            move || Arc::clone(&ev)
        }),
    );
    let _lane_guard = test_support::lane_consumer_exclusive();
    let (request, _rx) = dummy_request("model probe", false);
    let outcome = harness
        .execute_with_model(&request, Some("vendor/imaginary-model"))
        .expect("admitted, exit 0");
    assert_eq!(outcome.final_text, "ok-model");
    let r = record.lock().unwrap();
    assert_eq!(r.exec_calls.len(), 1, "exactly one contained exec");
    assert!(
        r.exec_calls[0]
            .1
            .iter()
            .all(|arg| !arg.contains("imaginary")),
        "v1 argv is model-free by deferral, got: {:?}",
        r.exec_calls[0].1
    );
}
