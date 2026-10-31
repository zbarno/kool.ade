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
    planning_root: std::path::PathBuf,
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
        if self.generation {
            assert!(req.system_instructions.contains("understand WHY"));
        } else {
            assert!(req.system_instructions.contains("Planning Analyst"));
            assert!(req.prompt_body.contains("understand WHY"));
        }
        let text = if !self.generation {
            assert!(
                req.prompt_body
                    .contains("Task generation is NOT authorized")
            );
            include_str!("../fixtures/interview-ready.json")
        } else {
            assert!(req.prompt_body.contains("The user explicitly approved"));
            if req.prompt_body.contains("ONLY detailed story 2 of 2") {
                let directory = self.planning_root.join("planning/tasks/saved-searches");
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
                let store = koolade::artifacts::planning_store::PlanningStore::new(
                    uuid::Uuid::nil(),
                    &self.planning_root,
                    koolade::artifacts::planning_store::StoreMode::ManagedLocal,
                );
                let state = PlannerState::load_with_store(&req.repo_root, &store).unwrap();
                let docs = { koolade::artifacts::task_docs::load_latest(&store, &state.workflow) };
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
                    include_str!("../fixtures/task-outline.json")
                }
                1 => {
                    assert!(req.prompt_body.contains("ONLY detailed story 1 of 2"));
                    include_str!("../fixtures/task-story-1.json")
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
                    include_str!("../fixtures/task-story-2.json")
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
pub(super) fn run(state: PlannerState, generation: bool, failure_mode: u8) -> TurnOutcome {
    let calls = Arc::new(AtomicUsize::new(0));
    let planning_root = state.planning_store.root.clone();
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
            planning_root,
        }),
    );
    // The workflow advances across worker threads and repository-backed
    // persistence; retain a bounded three-minute window under parallel nextest.
    let deadline = Instant::now() + Duration::from_secs(180);
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
