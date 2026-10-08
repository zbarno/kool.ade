use super::*;

struct BareNestedCheckAgent;

impl AiHarness for BareNestedCheckAgent {
    fn label(&self) -> String {
        "bare nested check fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        assert_eq!(request.mode, crate::harness::ExecutionMode::Implementation);
        fs::write(request.repo_root.join("implemented.txt"), "implemented\n").unwrap();
        Ok(outcome(serde_json::json!({
            "schemaVersion":2,
            "status":"complete",
            "blocker_disposition":"none",
            "summary":"Implemented the requested behavior.",
            "acceptance_criteria":[{"criterion":"File contains implemented.","evidence":"implemented.txt contains the expected content."}],
            "verification":["test -f marker"],
            "remaining":[]
        })))
    }
}

#[test]
fn bare_nested_check_report_uses_the_scoped_command_after_baseline_verification() {
    let s = Sandbox::new();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    s.advance_remote();
    let peer = s.root.join("peer");
    fs::create_dir_all(peer.join("Source")).unwrap();
    fs::write(
        peer.join("Source/AGENTS.md"),
        "## Quality Gates\n\n- Backend tests: `test -f marker`\n",
    )
    .unwrap();
    fs::write(peer.join("Source/marker"), "ready\n").unwrap();
    s.git(&peer, &["add", "Source"]);
    s.git(
        &peer,
        &["commit", "-qm", "add nested validation instructions"],
    );
    s.git(&peer, &["push", "-q", "origin", "main"]);
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    let remote = s.git(&peer, &["rev-parse", "HEAD"]);
    let common = s.git(&s.repo, &["merge-base", &local, &remote]);
    let dir = super::super::super::state_paths::state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    super::super::super::initial_reconciliation::save_plan(
        &dir,
        "main",
        &local,
        &remote,
        &common,
        &["cd -- 'Source' && test -f marker".into()],
    )
    .unwrap();
    set_saved_plan_verified_checks(&dir, &remote);

    let state = run_with_agent(&s, &BareNestedCheckAgent, None).unwrap();

    assert!(state.task_repository.join("implemented.txt").exists());
    assert_scoped_check_succeeded(&dir);
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("verified-report.json")).unwrap()).unwrap();
    assert_eq!(
        report["verification"],
        serde_json::json!(["cd -- 'Source' && test -f marker"])
    );
}

#[test]
fn verified_legacy_plan_refreshes_pending_checks_without_touching_task_edits() {
    let s = Sandbox::new();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    s.advance_remote();
    let peer = s.root.join("peer");
    fs::create_dir_all(peer.join("Source")).unwrap();
    fs::write(
        peer.join("Source/AGENTS.md"),
        "## Quality Gates\n\n- Backend tests: `test -f marker`\n",
    )
    .unwrap();
    fs::write(peer.join("Source/marker"), "ready\n").unwrap();
    s.git(&peer, &["add", "Source"]);
    s.git(
        &peer,
        &["commit", "-qm", "add nested validation instructions"],
    );
    s.git(&peer, &["push", "-q", "origin", "main"]);
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    let remote = s.git(&peer, &["rev-parse", "HEAD"]);
    let common = s.git(&s.repo, &["merge-base", &local, &remote]);
    let dir = super::super::super::state_paths::state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    super::super::super::initial_reconciliation::save_plan(
        &dir,
        "main",
        &local,
        &remote,
        &common,
        &["test -f marker".into()],
    )
    .unwrap();
    set_saved_plan_legacy_verification(&dir, &remote);

    let first_agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    let mut state = run_with_agent(&s, &first_agent, None).unwrap();
    assert!(
        s.git(&state.task_repository, &["status", "--porcelain"])
            .is_empty(),
        "first implementation left a clean verified worktree"
    );
    set_saved_plan_legacy_verification(&dir, &remote);

    let clean_resume_agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    state = run_with_agent(&s, &clean_resume_agent, None).unwrap();
    assert_eq!(clean_resume_agent.calls.load(Ordering::SeqCst), 1);
    assert_scoped_check_succeeded(&dir);

    fs::write(
        state.task_repository.join("in-progress.txt"),
        "preserve me\n",
    )
    .unwrap();
    set_saved_plan_legacy_verification(&dir, &remote);

    let resume_agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };
    state = run_with_agent(&s, &resume_agent, None).unwrap();

    assert_eq!(
        fs::read_to_string(state.task_repository.join("in-progress.txt")).unwrap(),
        "preserve me\n"
    );
    assert_scoped_check_succeeded(&dir);
    let plan = super::super::super::initial_reconciliation::load_plan(&dir)
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.required_verification,
        ["cd -- 'Source' && test -f marker"]
    );
}

fn assert_scoped_check_succeeded(dir: &std::path::Path) {
    let evidence_path = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_task_verification_artifact(path))
        .max_by_key(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .unwrap();
    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence_path).unwrap()).unwrap();
    assert!(evidence.as_array().unwrap().iter().any(|entry| {
        entry["command"] == "cd -- 'Source' && test -f marker" && entry["error"].is_null()
    }));
}

fn is_task_verification_artifact(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with("-verification.json") && !name.starts_with("base-reconciliation-")
        })
}

fn set_saved_plan_legacy_verification(dir: &std::path::Path, verified_commit: &str) {
    let path = dir.join("base-reconciliation.json");
    let mut plan: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    plan["required_verification"] = serde_json::json!(["test -f marker"]);
    plan["verified_commit"] = serde_json::Value::String(verified_commit.into());
    plan["verification"] = serde_json::json!(["test -f marker"]);
    fs::write(path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();

    let report_path = dir.join("verified-report.json");
    if report_path.exists() {
        let mut report: serde_json::Value =
            serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
        report["verification"] = serde_json::json!(["test -f implemented.txt"]);
        fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

fn set_saved_plan_verified_checks(dir: &std::path::Path, verified_commit: &str) {
    let path = dir.join("base-reconciliation.json");
    let mut plan: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let scoped = "cd -- 'Source' && test -f marker";
    plan["required_verification"] = serde_json::json!([scoped]);
    plan["verified_commit"] = serde_json::Value::String(verified_commit.into());
    plan["verification"] = serde_json::json!([scoped]);
    fs::write(path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
}
