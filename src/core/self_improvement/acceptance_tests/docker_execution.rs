use super::support::{git, pursue_decision, self_improvement_run, uid};
use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{
        opportunity::{OpportunityExploreModeV1, OpportunitySaveResult, OpportunityStore},
        self_improvement::{
            SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementStore,
            TaskSourceAuthorization,
        },
    },
};
use std::{process::Command, time::Duration};

/// Docker backend smoke test for task-clone mounting and source preservation.
/// The production dispatch authorization hook and cancellation fence are covered
/// by `production_dispatch_verifier_binds_approved_plan_task_source_and_revocation`.
/// Requires Linux and CI's rootless Docker Engine; setup is one local Git source,
/// store, clone, and command. A unit test cannot prove host-clone isolation.
#[test]
fn pursued_self_improvement_executes_in_its_pinned_docker_task_clone() {
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping self-improvement Docker E2E; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1");
        return;
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("skipping self-improvement Docker E2E; it requires Linux");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let operator_home = temp.path().join("operator-state");
    environment.set("KOOLADE_HOME", &operator_home);
    environment.remove("DOCKER_CONTEXT");
    let rootless_socket = format!(
        "unix://{}/docker.sock",
        std::env::var("XDG_RUNTIME_DIR").expect("rootless Docker requires XDG_RUNTIME_DIR")
    );
    assert_eq!(
        std::env::var("DOCKER_HOST").as_deref(),
        Ok(rootless_socket.as_str()),
        "the Docker E2E must use the approved per-user rootless socket"
    );
    let engine = Command::new("docker")
        .args(["info", "--format", "{{json .SecurityOptions}}"])
        .output()
        .expect("Docker CLI must be installed for the opt-in backend E2E");
    assert!(
        engine.status.success()
            && String::from_utf8_lossy(&engine.stdout).contains("name=rootless"),
        "Docker E2E refuses a non-rootless or unavailable engine: {}",
        String::from_utf8_lossy(&engine.stderr)
    );

    let source = temp.path().join("product-source");
    std::fs::create_dir_all(&source).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(source.join("product.txt"), "before\n").unwrap();
    git(&source, &["add", "product.txt"]);
    git(
        &source,
        &["commit", "--quiet", "-m", "synthetic product baseline"],
    );
    let source_sha = git(&source, &["rev-parse", "HEAD"]);
    let source_status = git(&source, &["status", "--porcelain"]);

    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let repository_uid =
        crate::core::project_repos::stable_uid(&workspace_uid, "repository", "root");
    let opportunities = OpportunityStore::new(&planning);
    let defaults = opportunities.discovery_settings().unwrap();
    opportunities
        .save_discovery_settings(
            &crate::core::opportunity::OpportunityDiscoverySettingsV1 {
                exploration: OpportunityExploreModeV1::OptedIn,
                ..defaults.clone()
            },
            defaults.revision,
        )
        .unwrap();
    let mut proposal = super::support::proposal(&source_sha);
    proposal.workspace_uid = workspace_uid;
    proposal.target.repository_uid = repository_uid.clone();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("first synthetic product proposal should be created")
    };
    let decision = pursue_decision(&proposal);
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();

    let runs = SelfImprovementStore::new(&planning);
    let run = runs
        .create_from_pursued(
            &opportunities,
            &decision.uid,
            self_improvement_run(&decision.uid, &proposal, &source_sha),
        )
        .unwrap();
    assert_eq!(run.phase, Phase::Planning);
    let task_uid = uid();
    let source_ref = "koolade/self-improvement-docker-fixture";
    let ready = runs
        .apply_event(
            &run.run_uid,
            run.revision,
            &uid(),
            Event::PlanApproved {
                approval_sha256: "d".repeat(64),
                capability_id: Some("self-improvement-lifecycle".into()),
                review_uid: uid(),
                source_ref: source_ref.into(),
                destination_ref: "main".into(),
                task_uids: vec![task_uid.clone()],
                owner_task_uid: task_uid.clone(),
                parent_task_uids: vec![task_uid.clone()],
            },
        )
        .unwrap();
    assert_eq!(ready.phase, Phase::Ready);
    runs.authorize_task_start(
        &ready.feature_uid,
        Some(&task_uid),
        &repository_uid,
        &proposal.target.canonical_remote,
    )
    .unwrap();
    runs.authorize_task_source(TaskSourceAuthorization {
        feature_uid: &ready.feature_uid,
        task_uid: &task_uid,
        repository_uid: &repository_uid,
        repository_remote: &proposal.target.canonical_remote,
        source_ref: Some(source_ref),
        source_sha: Some(&source_sha),
        destination_ref: Some("main"),
        isolated_clone: true,
    })
    .unwrap();
    let implementing = runs
        .apply_event(
            &ready.run_uid,
            ready.revision,
            &uid(),
            Event::CloneAllocated { clone_uid: uid() },
        )
        .unwrap();
    assert_eq!(implementing.phase, Phase::Implementing);

    let project_slug = crate::persistence::project_slug(&source.canonicalize().unwrap());
    let task_clone = operator_home
        .join("projects")
        .join(project_slug)
        .join("task-repositories")
        .join("root")
        .join(format!("self-improvement-{}", &task_uid[..8]));
    std::fs::create_dir_all(task_clone.parent().unwrap()).unwrap();
    let clone_output = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks", "--"])
        .arg(&source)
        .arg(&task_clone)
        .output()
        .unwrap();
    assert!(
        clone_output.status.success(),
        "task clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    git(&task_clone, &["checkout", "--quiet", "-b", source_ref]);

    let workspace = crate::execution::contract::WorkspaceRef {
        project_id: planning.project_id.hyphenated().to_string(),
        task_id: task_uid.clone(),
        source_sha: source_sha.clone(),
    };
    let session = crate::execution::session::ExecutionSession::connect_local(
        workspace,
        &task_clone,
        crate::execution::contract::ExecutionModeV1::Implementation,
        crate::execution::docker::images::RuntimeProfile::Rust,
        crate::execution::contract::ResourceBudgetV1::default(),
    )
    .unwrap_or_else(|error| panic!("rootless Docker task session failed: {error}"));
    let host_only = temp.path().join("host-only-sentinel");
    std::fs::write(&host_only, "must not be mounted\n").unwrap();
    let command = format!(
        "test ! -e '{}' && printf 'implemented\\n' > product.txt",
        host_only.display()
    );
    let output = session
        .run_tool_command(&command, Duration::from_secs(30))
        .unwrap_or_else(|error| panic!("Docker command failed: {error}"));
    assert_eq!(output.result.exit_code, Some(0), "{}", output.stderr);
    assert_eq!(
        std::fs::read_to_string(task_clone.join("product.txt")).unwrap(),
        "implemented\n"
    );
    assert!(host_only.is_file());
    assert_eq!(
        std::fs::read_to_string(source.join("product.txt")).unwrap(),
        "before\n",
        "implementation must not modify the product source checkout"
    );
    assert_eq!(git(&source, &["status", "--porcelain"]), source_status);
    drop(session);

    let persisted = runs.load(&implementing.run_uid).unwrap();
    assert_eq!(persisted.phase, Phase::Implementing);
    assert_eq!(persisted.source_sha, source_sha);
    assert_eq!(persisted.task_uids, vec![task_uid]);
}
