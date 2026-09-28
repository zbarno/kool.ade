use packet::{
    core::{
        project_repos::{ProjectManifest, Repository, map_local_checkout},
        state::PlannerState,
        turn::{TurnController, TurnEvt, TurnInputs, TurnOutcome},
        workflow::{TurnPurpose, Workflow, feature_contract},
    },
    error::AppError,
    harness::{AiHarness, HarnessOutcome, PlanningRequest, TurnEnvelope},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
fn init(repo: &Path, remote: &str) -> String {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.name", "Fixture"]);
    git(repo, &["config", "user.email", "fixture@example.test"]);
    git(repo, &["remote", "add", "origin", remote]);
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-qm", "baseline"]);
    git(repo, &["rev-parse", "HEAD"])
}

struct MultiHarness;
impl AiHarness for MultiHarness {
    fn label(&self) -> String {
        "multi-repo fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert!(
            request
                .prompt_body
                .contains("APPROVED FEATURE SPECIFICATION")
        );
        assert!(request.prompt_body.contains("repository_bases"));
        assert!(request.prompt_body.contains("product_modules"));
        let mut value: serde_json::Value =
            if request.prompt_body.contains("ONLY detailed story 1 of 2") {
                serde_json::from_str(include_str!("fixtures/task-story-1.json")).unwrap()
            } else if request.prompt_body.contains("OUTLINE FIRST") {
                serde_json::from_str(include_str!("fixtures/task-outline.json")).unwrap()
            } else {
                assert!(request.prompt_body.contains("ONLY detailed story 2 of 2"));
                serde_json::from_str(include_str!("fixtures/task-story-2.json")).unwrap()
            };
        if let Some(story) = value
            .get_mut("task_stories")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|stories| stories.first_mut())
        {
            story["definition_of_done"] = serde_json::json!(["Behavior and tests pass."]);
        }
        if let Some(outline) = value
            .get_mut("task_outline")
            .and_then(serde_json::Value::as_array_mut)
        {
            outline[0]["target_repository"] = "api".into();
            outline[1]["target_repository"] = "web".into();
        }
        Ok(HarnessOutcome {
            final_text: value.to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn approved_feature_generates_dependent_tasks_for_distinct_repositories() {
    let root = std::env::temp_dir().join(format!(
        "packet_multi_feature_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let planning = root.join("planning-root");
    let api = root.join("api");
    let web = root.join("web");
    let api_base = init(&api, "git@example.test:product/api.git");
    let web_base = init(&web, "git@example.test:product/web.git");
    init(&planning, "git@example.test:product/planning.git");
    packet::artifacts::migration::run(&planning).unwrap();
    let mut state = PlannerState::load(&planning).unwrap();
    state.bootstrap_missing().unwrap();
    std::fs::create_dir_all(
        planning.join(".kool-ade-packet/planning/changes/CHG-001-saved-searches"),
    )
    .unwrap();
    let feature_content = "# CHG-001: Saved searches\n\n**Status:** Ready\n\n**Affected repositories:** api, web\n\n## Intent\n\nAnalysts resume searches.\n\n## Current Behavior\n\nSearches are transient.\n\n## Desired Behavior\n\nSearches persist and are selectable.\n\n## Scope\n\nAPI persistence and web picker.\n\n## Affected Product Areas\n\n`product:current-capabilities`; repositories `api`, `web`.\n\n## Requirements\n\nSave and restore named searches.\n\n## Decisions and Assumptions\n\nUse versioned records.\n\n## Acceptance Criteria\n\nA saved search survives restart and can be selected.\n";
    let identified = packet::domain::ArtifactIdentity::preserve_markdown(
        feature_content,
        None,
        "CHG-001",
        "Saved searches",
    )
    .unwrap();
    let identity = packet::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let feature = packet::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        packet::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(
        planning.join(".kool-ade-packet/planning/changes/CHG-001-saved-searches/specification.md"),
        &feature,
    )
    .unwrap();
    let manifest = ProjectManifest {
        repositories: vec![
            Repository {
                id: "root".into(),
                role: "Planning root".into(),
                remote: "git@example.test:product/planning.git".into(),
                display_name: None,
            },
            Repository {
                id: "api".into(),
                role: "Backend API".into(),
                remote: "git@example.test:product/api.git".into(),
                display_name: None,
            },
            Repository {
                id: "web".into(),
                role: "Web client".into(),
                remote: "git@example.test:product/web.git".into(),
                display_name: None,
            },
        ],
    };
    std::fs::write(
        planning.join(packet::artifacts::layout::canonical::PROJECT_MANIFEST),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    map_local_checkout(&planning, "api", &api).unwrap();
    map_local_checkout(&planning, "web", &web).unwrap();
    let ready: TurnEnvelope =
        serde_json::from_str(include_str!("fixtures/interview-ready.json")).unwrap();
    let mut workflow = Workflow {
        brief: ready.interview,
        reviewed_specification: Some(feature.clone()),
        ..Default::default()
    };
    packet::artifacts::task_docs::save_workflow(&planning, &workflow).unwrap();
    let unapproved = PlannerState::load(&planning).unwrap();
    let controller = TurnController::start(
        TurnInputs {
            state: unapproved,
            user_message: "Generate tasks".into(),
            recent_chat: Vec::new(),
            purpose: TurnPurpose::GenerateTasks,
            comparison_feature: None,
        },
        Box::new(MultiHarness),
    );
    let denied = loop {
        if let Some(TurnEvt::Done(result)) = controller.poll(Duration::from_millis(100)) {
            break result;
        }
    };
    assert!(matches!(*denied, TurnOutcome::Rejected { .. }));
    assert!(
        !planning
            .join(packet::artifacts::layout::canonical::TASKS)
            .exists()
    );
    workflow
        .approved_features
        .insert("CHG-001".into(), feature_contract(&feature));
    packet::artifacts::task_docs::save_workflow(&planning, &workflow).unwrap();
    git(&planning, &["add", "-A"]);
    git(&planning, &["commit", "-qm", "approve feature"]);
    let state = PlannerState::load(&planning).unwrap();
    assert!(state.workflow.ready(state.planning_contract()));
    let controller = TurnController::start(
        TurnInputs {
            state,
            user_message: "Generate approved tasks".into(),
            recent_chat: Vec::new(),
            purpose: TurnPurpose::GenerateTasks,
            comparison_feature: None,
        },
        Box::new(MultiHarness),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = loop {
        assert!(
            Instant::now() < deadline,
            "multi-repository task generation timed out"
        );
        if let Some(TurnEvt::Done(result)) = controller.poll(Duration::from_millis(100)) {
            break result;
        }
    };
    let TurnOutcome::Applied {
        state,
        commit_result,
        ..
    } = *result
    else {
        panic!("generation did not apply: {result:#?}");
    };
    assert!(commit_result.is_ok());
    let batch = state.workflow.task_batches.last().unwrap();
    assert_eq!(batch.count, 2);
    let dir = planning.join(&batch.directory);
    let contract: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("contract.json")).unwrap()).unwrap();
    assert_eq!(contract["featureId"], "CHG-001");
    assert_eq!(contract["repositoryBases"]["api"], api_base);
    assert_eq!(contract["repositoryBases"]["web"], web_base);
    assert!(
        contract["productModules"]
            .get("current-capabilities")
            .is_some()
    );
    assert!(
        !std::fs::read_to_string(dir.join("contract.json"))
            .unwrap()
            .contains(api.to_str().unwrap())
    );
    let first =
        std::fs::read_to_string(dir.join("CHG-001-TASK-persist-named-search-filters.md")).unwrap();
    let second =
        std::fs::read_to_string(dir.join("CHG-001-TASK-build-the-saved-search-picker.md")).unwrap();
    assert!(first.contains("Repository: api"));
    assert!(second.contains("Repository: web"));
    assert!(second.contains("CHG-001-TASK-persist-named-search-filters.md"));
    assert!(
        !api.join(packet::artifacts::layout::canonical::TASKS)
            .exists()
    );
    assert!(
        !web.join(packet::artifacts::layout::canonical::TASKS)
            .exists()
    );
    let private = packet::persistence::project_dir(&packet::persistence::project_slug(
        &planning.canonicalize().unwrap(),
    ));
    let _ = std::fs::remove_dir_all(private);
    let _ = std::fs::remove_dir_all(root);
}
