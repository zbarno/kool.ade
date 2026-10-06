use super::*;
use crate::harness::{ExecutionMode, ModelCallUsage};
use std::{
    env,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

static HOME_LOCK: Mutex<()> = Mutex::new(());

struct RestoreHome(Option<std::ffi::OsString>);
impl Drop for RestoreHome {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => unsafe { env::set_var("KOOLADE_HOME", value) },
            None => unsafe { env::remove_var("KOOLADE_HOME") },
        }
    }
}

struct UsageHarness {
    calls: AtomicUsize,
    barrier: Option<Arc<std::sync::Barrier>>,
}

impl AiHarness for UsageHarness {
    fn label(&self) -> String {
        "fake 1.2".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("1.2".into())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let call = ModelCallUsage {
            call_id: "provider-response".into(),
            provider: Some("example".into()),
            api: Some("chat".into()),
            model: Some("model-a".into()),
            input_tokens: Some(100),
            output_tokens: Some(20),
            total_tokens: Some(120),
            estimated_cost_usd_micros: Some(250),
            started_at: Some(chrono::Utc::now()),
            ended_at: Some(chrono::Utc::now()),
            duration_millis: Some(12),
            ..Default::default()
        };
        let _ = request.progress_tx.send(LiveProgress {
            model_calls: vec![call],
            ..Default::default()
        });
        if let Some(barrier) = &self.barrier {
            barrier.wait();
            std::thread::sleep(Duration::from_millis(25));
        }
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(crate::error::AppError::Other(
                "failure after model response".into(),
            ))
        } else {
            Ok(HarnessOutcome {
                final_text: "done".into(),
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
}

fn request(progress_tx: mpsc::Sender<LiveProgress>) -> PlanningRequest {
    PlanningRequest {
        mode: ExecutionMode::Implementation,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: env::temp_dir(),
        prompt_body: "synthetic task".into(),
        system_instructions: String::new(),
        timeout: Duration::from_secs(3),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn failed_attempt_usage_is_retained_and_retry_appends_a_distinct_record() {
    let _lock = HOME_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = env::temp_dir().join(format!("koolade_impl_telemetry_{}", uuid::Uuid::new_v4()));
    let _restore = RestoreHome(env::var_os("KOOLADE_HOME"));
    unsafe { env::set_var("KOOLADE_HOME", &home) };
    let root = home.join("synthetic-repo");
    std::fs::create_dir_all(&root).unwrap();
    let harness = UsageHarness {
        calls: AtomicUsize::new(0),
        barrier: None,
    };
    let capture = CaptureHarness::new(
        &harness,
        &root,
        "# Synthetic task\n\nFeature: Example F23\n",
        None,
        Some("task-23"),
    );

    let (tx, _rx) = mpsc::channel();
    assert!(capture.execute(&request(tx)).is_err());
    let (tx, _rx) = mpsc::channel();
    assert!(capture.execute(&request(tx)).is_ok());

    let slug = crate::persistence::project_slug(&root.canonicalize().unwrap());
    let (records, skipped) = crate::persistence::telemetry::load(&slug);
    assert_eq!(skipped, 0);
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.task.as_deref() == Some("task-23"))
    );
    assert!(records.iter().all(|record| record.batch.is_none()));
    assert_eq!(records[0].outcome, "failed");
    assert_eq!(records[0].input_tokens, Some(100));
    assert_eq!(records[0].estimated_cost_usd_micros, Some(250));
    assert_eq!(records[1].outcome, "completed");
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn telemetry_write_failure_is_visible_but_does_not_abort_the_harness_result() {
    let _lock = HOME_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = env::temp_dir().join(format!(
        "koolade_impl_telemetry_blocked_{}",
        uuid::Uuid::new_v4()
    ));
    let _restore = RestoreHome(env::var_os("KOOLADE_HOME"));
    std::fs::write(&home, "not a directory").unwrap();
    unsafe { env::set_var("KOOLADE_HOME", &home) };
    let root = env::temp_dir().join(format!("koolade_impl_repo_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let harness = UsageHarness {
        calls: AtomicUsize::new(1),
        barrier: None,
    };
    let capture = CaptureHarness::new(&harness, &root, "# Synthetic task", None, Some("task-24"));
    let (tx, rx) = mpsc::channel();
    assert!(capture.execute(&request(tx)).is_ok());
    let updates = rx.try_iter().collect::<Vec<_>>();
    assert!(updates.iter().any(|progress| {
        progress
            .activity
            .as_deref()
            .is_some_and(|text| text.contains("telemetry could not be saved"))
    }));
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(home);
}

#[test]
fn concurrent_tasks_in_one_repository_keep_separate_attributed_histories() {
    let _lock = HOME_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = env::temp_dir().join(format!("koolade_impl_parallel_{}", uuid::Uuid::new_v4()));
    let _restore = RestoreHome(env::var_os("KOOLADE_HOME"));
    unsafe { env::set_var("KOOLADE_HOME", &home) };
    let root = home.join("shared-repository");
    std::fs::create_dir_all(&root).unwrap();
    let harness = UsageHarness {
        calls: AtomicUsize::new(1),
        barrier: Some(Arc::new(std::sync::Barrier::new(2))),
    };
    let first = CaptureHarness::new(
        &harness,
        &root,
        "# Synthetic task\n\nFeature: Example F23\n",
        None,
        Some("task-alpha"),
    );
    let second = CaptureHarness::new(
        &harness,
        &root,
        "# Synthetic task\n\nFeature: Example F23\n",
        None,
        Some("task-beta"),
    );
    let (active_tx, _active_rx) = mpsc::channel();
    let first_active = ActiveSpan::new(
        &root,
        "# Synthetic task\n\nFeature: Example F23\n",
        None,
        Some("task-alpha"),
        "session-alpha",
        active_tx.clone(),
    );
    let second_active = ActiveSpan::new(
        &root,
        "# Synthetic task\n\nFeature: Example F23\n",
        None,
        Some("task-beta"),
        "session-beta",
        active_tx,
    );
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            let (tx, _rx) = mpsc::channel();
            first.execute(&request(tx)).unwrap();
        });
        let b = scope.spawn(|| {
            let (tx, _rx) = mpsc::channel();
            second.execute(&request(tx)).unwrap();
        });
        a.join().unwrap();
        b.join().unwrap();
    });
    drop((first_active, second_active));

    let slug = crate::persistence::project_slug(&root.canonicalize().unwrap());
    let (records, skipped) = crate::persistence::telemetry::load(&slug);
    assert_eq!(skipped, 0);
    assert_eq!(records.len(), 4);
    for task in ["task-alpha", "task-beta"] {
        let attributed = records
            .iter()
            .filter(|record| record.task.as_deref() == Some(task))
            .collect::<Vec<_>>();
        assert_eq!(attributed.len(), 2);
        let invocation = attributed
            .iter()
            .find(|record| record.phase.as_deref() != Some("active_implementation"))
            .unwrap();
        let active = attributed
            .iter()
            .find(|record| record.phase.as_deref() == Some("active_implementation"))
            .unwrap();
        assert!(
            invocation
                .feature
                .as_deref()
                .is_some_and(|v| v.contains("F23"))
        );
        assert_eq!(invocation.input_tokens, Some(100));
        assert_eq!(invocation.estimated_cost_usd_micros, Some(250));
        assert!(active.duration_millis.unwrap_or_default() >= 20);
        let report = crate::persistence::telemetry::report::for_task(&records, task);
        assert_eq!(report.model_calls, Some(1));
        assert_eq!(report.total_tokens, Some(120));
        assert_eq!(report.estimated_cost_usd_micros, Some(250));
        assert!(report.active_implementation_millis >= 20);
    }
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(home);
}
