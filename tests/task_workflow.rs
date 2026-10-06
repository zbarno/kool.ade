use koolade::core::{
    state::PlannerState,
    turn::{TurnController, TurnEvt, TurnInputs, TurnOutcome},
    workflow::TurnPurpose,
};
use koolade::error::AppError;
use koolade::harness::{AiHarness, HarnessOutcome, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

struct FixtureHarness {
    calls: Arc<AtomicUsize>,
    generation: bool,
    failure_mode: u8,
}
impl AiHarness for FixtureHarness {
    fn label(&self) -> String {
        "workflow-fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(req.system_instructions.contains("understand WHY"));
        let text = if !self.generation {
            assert!(
                req.prompt_body
                    .contains("Task generation is NOT authorized")
            );
            include_str!("fixtures/interview-ready.json")
        } else {
            assert!(req.prompt_body.contains("The user explicitly approved"));
            if req.prompt_body.contains("ONLY detailed story 2 of 2") {
                let directory = req
                    .repo_root
                    .join(".koolade-packet/planning/tasks/saved-searches");
                assert!(
                    directory
                        .join("001-persist-named-search-filters.md")
                        .exists(),
                    "first story must be saved before requesting the second"
                );
                assert!(
                    !directory
                        .join("002-build-the-saved-search-picker.md")
                        .exists()
                );
                assert!(
                    std::fs::read_to_string(directory.join("README.md"))
                        .unwrap()
                        .contains("In progress — 1 of 2")
                );
                let state = PlannerState::load(&req.repo_root).unwrap();
                let docs =
                    koolade::artifacts::task_docs::load_latest(&req.repo_root, &state.workflow);
                assert!(
                    docs.iter()
                        .any(|d| d.path.ends_with("001-persist-named-search-filters.md")),
                    "saved story must be discoverable by the task panel before completion"
                );
                assert!(state.workflow.task_batches.is_empty());
            }
            if self.failure_mode == 1 && n == 2 {
                return Err(AppError::Other("second story failed".into()));
            }
            if self.failure_mode == 2 && n == 2 {
                req.cancel.store(true, Ordering::SeqCst);
            }
            match if req.prompt_body.contains("OUTLINE FIRST") {
                0
            } else if req.prompt_body.contains("ONLY detailed story 1 of 2") {
                1
            } else {
                2
            } {
                0 => {
                    assert!(req.prompt_body.contains("OUTLINE FIRST"));
                    include_str!("fixtures/task-outline.json")
                }
                1 => {
                    assert!(req.prompt_body.contains("ONLY detailed story 1 of 2"));
                    include_str!("fixtures/task-story-1.json")
                }
                2 => {
                    assert!(req.prompt_body.contains("ONLY detailed story 2 of 2"));
                    assert!(
                        req.prompt_body
                            .contains("=== COMPLETED DEPENDENCY STORIES ===")
                    );
                    assert!(
                        req.prompt_body
                            .contains("versioned saved-search persistence contract")
                    );
                    include_str!("fixtures/task-story-2.json")
                }
                _ => panic!("unexpected model call"),
            }
        };
        let mut text = text.to_owned();
        if self.generation
            && let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(story) = value
                .get_mut("task_stories")
                .and_then(serde_json::Value::as_array_mut)
                .and_then(|stories| stories.first_mut())
        {
            story["definition_of_done"] = serde_json::json!(["Behavior and tests pass."]);
            text = value.to_string();
        }
        if self.generation && (self.failure_mode == 3 && n == 1 || self.failure_mode == 6 && n >= 1)
        {
            text = r#"{"task_stories":[{"title":"Generic task"}]}"#.into();
        }
        if self.generation && self.failure_mode == 3 && n == 2 {
            assert!(req.prompt_body.contains("REPAIR THIS RESPONSE"));
            assert!(req.prompt_body.contains("acceptance"));
        }
        if self.generation && self.failure_mode == 4 && n == 1 {
            let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["task_stories"][0]["title"] =
                "A harmless paraphrase of the approved title".into();
            value["task_stories"][0]["purpose"] = "A differently worded purpose".into();
            text = value.to_string();
        }
        Ok(HarnessOutcome {
            final_text: text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
fn run(state: PlannerState, generation: bool, failure_mode: u8) -> TurnOutcome {
    let calls = Arc::new(AtomicUsize::new(0));
    let ctrl = TurnController::start(
        TurnInputs {
            state,
            user_message: if generation {
                "Yes, generate tasks."
            } else {
                "The goal and scope are agreed."
            }
            .into(),
            recent_chat: Vec::new(),
            purpose: if generation {
                TurnPurpose::GenerateTasks
            } else {
                TurnPurpose::Interview
            },
            comparison_feature: None,
        },
        Box::new(FixtureHarness {
            calls: calls.clone(),
            generation,
            failure_mode,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "workflow timed out");
        if let Some(TurnEvt::Done(outcome)) = ctrl.poll(Duration::from_millis(50)) {
            assert_eq!(
                calls.load(Ordering::SeqCst),
                if !generation || failure_mode == 5 {
                    1
                } else if failure_mode == 3 {
                    4
                } else if failure_mode == 6 {
                    7
                } else {
                    3
                },
                "failure mode {failure_mode}"
            );
            return *outcome;
        }
    }
}
#[test]
fn interview_approval_multicall_generation_commit_and_failure_recovery() {
    for failure_mode in [0, 1, 2, 3, 4, 6] {
        let root = std::env::temp_dir().join(format!(
            "koolade_full_workflow_{}_{failure_mode}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let mut state = PlannerState::load(&root).unwrap();
        state.bootstrap_missing().unwrap();
        let TurnOutcome::Applied {
            state,
            commit_result,
            ..
        } = run(state, false, 0)
        else {
            panic!("interview failed")
        };
        assert!(commit_result.is_ok());
        assert!(state.workflow.ready(state.spec_text.as_deref()));
        assert!(!root.join(".koolade-packet/planning/tasks").exists());
        let outcome = run(*state, true, failure_mode);
        if failure_mode == 6 {
            let TurnOutcome::HarnessFailed { error, .. } = outcome else {
                panic!("invalid stories must fail closed")
            };
            assert!(
                error.detail().contains("after 6 attempts"),
                "unexpected generation failure: {}",
                error.detail()
            );
            assert!(
                !root
                    .join(
                        ".koolade-packet/planning/tasks/saved-searches/001-persist-named-search-filters.md"
                    )
                    .exists()
            );
        } else if failure_mode == 1 || failure_mode == 2 {
            assert!(matches!(outcome, TurnOutcome::HarnessFailed { .. }));
            let first = root.join(
                ".koolade-packet/planning/tasks/saved-searches/001-persist-named-search-filters.md",
            );
            let saved = std::fs::read_to_string(&first).unwrap();
            let reloaded = PlannerState::load(&root).unwrap();
            assert!(
                reloaded.workflow.ready(reloaded.spec_text.as_deref()),
                "failed generation can be retried"
            );
            if failure_mode == 1 {
                let manifest: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(root.join(
                        ".koolade-packet/planning/tasks/saved-searches/.koolade-progress.json",
                    ))
                    .unwrap(),
                )
                .unwrap();
                let envelope: koolade::harness::TurnEnvelope =
                    serde_json::from_str(include_str!("fixtures/task-story-1.json")).unwrap();
                let batch = koolade::core::workflow::TaskBatch {
                    brief: reloaded.workflow.brief.clone().unwrap(),
                    specification: reloaded.spec_text.clone().unwrap(),
                    feature_id: None,
                    contract: None,
                    branch_targets: None,
                    task_routing: Default::default(),
                    stories: envelope.task_stories.unwrap(),
                };
                let edited = format!("{saved}\nUser correction\n");
                std::fs::write(&first, &edited).unwrap();
                assert!(
                    koolade::artifacts::task_docs::save_progress(
                        &root,
                        manifest["run"].as_str().unwrap(),
                        &batch,
                        2
                    )
                    .is_err()
                );
                assert_eq!(std::fs::read_to_string(&first).unwrap(), edited);
                std::fs::write(&first, &saved).unwrap();
            }
            let recovered = run(reloaded, true, 5);
            assert!(
                matches!(recovered, TurnOutcome::Applied { .. }),
                "retry must resume the second story with one model call"
            );
            assert_eq!(std::fs::read_to_string(first).unwrap(), saved);
            assert!(
                !root
                    .join(".koolade-packet/planning/tasks/saved-searches-02")
                    .exists()
            );
            assert!(
                std::fs::read_to_string(
                    root.join(".koolade-packet/planning/tasks/saved-searches/README.md")
                )
                .unwrap()
                .contains("Status: Complete")
            );
        } else {
            let TurnOutcome::Applied {
                state,
                commit_result,
                ..
            } = outcome
            else {
                panic!("task generation failed")
            };
            assert!(commit_result.is_ok());
            assert_eq!(state.workflow.task_batches[0].count, 2);
            let batch_ref = &state.workflow.task_batches[0];
            let batch_identity = batch_ref.identity.as_ref().expect("batch UID persisted");
            assert!(batch_identity.display_id.starts_with("BATCH-"));
            let batch_index =
                std::fs::read_to_string(root.join(&batch_ref.directory).join("README.md")).unwrap();
            assert_eq!(
                koolade::domain::ArtifactIdentity::from_markdown(&batch_index)
                    .unwrap()
                    .unwrap(),
                *batch_identity
            );
            let first_story = std::fs::read_to_string(
                root.join(&batch_ref.directory)
                    .join("001-persist-named-search-filters.md"),
            )
            .unwrap();
            let story_identity = koolade::domain::ArtifactIdentity::from_markdown(&first_story)
                .unwrap()
                .unwrap();
            assert_ne!(story_identity.uid, batch_identity.uid);
            assert!(
                story_identity
                    .display_id
                    .starts_with("001-persist-named-search-filters")
            );
            let tracked = std::process::Command::new("git")
                .args(["ls-tree", "-r", "--name-only", "HEAD"])
                .current_dir(&root)
                .output()
                .unwrap();
            let paths = String::from_utf8(tracked.stdout).unwrap();
            assert!(paths.contains(
                ".koolade-packet/planning/tasks/saved-searches/001-persist-named-search-filters.md"
            ));
            assert!(paths.contains(
                ".koolade-packet/planning/tasks/saved-searches/002-build-the-saved-search-picker.md"
            ));
            assert!(
                paths.contains(".koolade-packet/planning/tasks/saved-searches/specification.md")
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
