use super::*;

struct LivePi;
impl crate::harness::AiHarness for LivePi {
    fn label(&self) -> String {
        crate::harness::PiHarness.label()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        crate::harness::PiHarness.check_available()
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        let mut request = request.clone();
        request.reasoning_level = "off".into();
        crate::harness::PiHarness.execute(&request)
    }
}

#[test]
fn temporary_live_fixture_prepare_and_generate() {
    let Ok(root) = std::env::var("PACKET_APPROVAL_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let id = std::env::var("PACKET_APPROVAL_FEATURE").unwrap();
    let mut app = super::super::super::board_tests::fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        project.task_documents.clear();
    }
    app.task_harness = Some(Box::new(LivePi));
    let compare = std::env::var("PACKET_APPROVAL_PURPOSE").as_deref() == Ok("compare");
    let has_batch = match &app.screen {
        Screen::Connected(project) => has_current_task_batch(project),
        _ => false,
    };
    if compare {
        let mut comparison_saved = false;
        for _ in 0..3 {
            app.start_comparison_turn(&id);
            let ctx = egui::Context::default();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12 * 60 * 60);
            while app.conversation_busy() {
                app.tick(0.016, &ctx);
                assert!(
                    std::time::Instant::now() < deadline,
                    "live ComparePlans turn timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let state = crate::core::state::PlannerState::load(&root).unwrap();
            comparison_saved = state
                .active_features
                .iter()
                .find(|(feature_id, _)| feature_id == &id)
                .and_then(|(_, body)| crate::domain::ChangeMetadata::require_markdown(body).ok())
                .and_then(|metadata| metadata.plan_comparison)
                .is_some();
            if comparison_saved {
                break;
            }
        }
        assert!(
            comparison_saved,
            "Pi did not produce a valid comparison after three turns"
        );
    } else if !has_batch {
        if id == "F7" {
            let Screen::Connected(project) = &app.screen else {
                unreachable!()
            };
            assert!(
                crate::core::workflow::feature_approved(&root, &project.state.workflow, &id),
                "F7 must retain the explicit task-generation approval"
            );
            assert!(
                project
                    .state
                    .workflow
                    .ready(project.state.planning_contract())
            );
            app.start_turn_with_purpose(
                &format!("Generate task stories for approved feature {id}."),
                crate::core::workflow::TurnPurpose::GenerateTasks,
            );
        } else {
            app.approve_and_prepare_feature(&id);
        }
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12 * 60 * 60);
        while app.conversation_busy() {
            app.tick(0.016, &ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "live feature preparation/generation timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    assert!(matches!(&app.screen, Screen::Connected(_)));
    // TaskGeneration commits through its worker while Packet's visible
    // project snapshot may still be stale; reload the persisted authority.
    let state = crate::core::state::PlannerState::load(&root).unwrap();
    if compare {
        let (_, body) = state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == &id)
            .unwrap();
        let metadata = crate::domain::ChangeMetadata::require_markdown(body).unwrap();
        assert_eq!(metadata.schema_version, 2);
        assert_eq!(metadata.plan_comparison.unwrap().alternatives.len(), 2);
    } else {
        assert!(crate::core::workflow::feature_approved(
            &root,
            &state.workflow,
            &id
        ));
        let batch = if id == "F7" {
            let current = state
                .active_features
                .iter()
                .find(|(feature_id, _)| feature_id == &id)
                .expect("approved feature specification was not persisted");
            state
                .workflow
                .task_batches
                .iter()
                .rev()
                .filter(|batch| batch.feature.contains(&id))
                .find(|batch| {
                    crate::core::contract_snapshot::batch_contract_matches_feature(
                        &root,
                        &batch.directory,
                        &id,
                        &current.1,
                    )
                })
                .expect("no persisted F7 batch freezes the adopted plan")
        } else {
            state
                .workflow
                .task_batches
                .iter()
                .find(|batch| batch.feature.contains(&id))
                .expect("approved feature task batch was not persisted")
        };
        if id == "F7" {
            assert_ne!(
                batch.directory,
                ".kool-ade-packet/planning/tasks/F7-comparative-feature-plan-comparison-before-approval-05"
            );
        }
        if id == "CHG-007" {
            let directory = root.join(&batch.directory);
            let stories = std::fs::read_dir(&directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "md")
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("CHG-007-TASK-")
                })
                .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(stories.len(), batch.count);
            assert!(!stories.is_empty());
            for story in &stories {
                assert!(
                    story.len() <= 8_000,
                    "small-fix story is bloated: {} bytes",
                    story.len()
                );
                for section in [
                    "## Purpose",
                    "## Ticket goal",
                    "## Implementation steps",
                    "## Acceptance criteria",
                    "## Test plan",
                    "## Definition of done",
                ] {
                    assert!(story.contains(section), "missing {section}");
                }
                assert!(story.contains("src/ui/layout/task_details/"));
                assert!(story.contains("src/app/root/task_detail_tests.rs"));
                assert!(!story.to_ascii_lowercase().contains("css"));
                assert!(!story.contains("not located in sandbox"));
            }
            eprintln!(
                "Scenario B: batch {}, {} stories, sizes {:?} bytes",
                batch
                    .identity
                    .as_ref()
                    .map(|identity| identity.display_id.as_str())
                    .unwrap_or("legacy"),
                stories.len(),
                stories.iter().map(String::len).collect::<Vec<_>>()
            );
        }
    }
}
