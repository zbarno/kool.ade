use super::*;
use crate::core::workflow::{self, TurnPurpose};
use crate::ui::feature_approval::Action;

impl PacketApp {
    pub(super) fn available_feature_actions(&self, conversation: Option<&str>) -> Vec<Action> {
        let Screen::Connected(p) = &self.screen else {
            return Vec::new();
        };
        let related = conversation.and_then(|key| {
            p.state
                .items
                .iter()
                .chain(&p.state.resolved_items)
                .find(|item| item.conversation_key() == key)
                .and_then(|item| item.feature_id.as_deref())
                .or_else(|| {
                    p.task_documents
                        .iter()
                        .find(|doc| doc.path == key)
                        .and_then(|doc| {
                            doc.text
                                .lines()
                                .find_map(|line| line.strip_prefix("Feature ID: "))
                        })
                })
        });
        p.state
            .active_features
            .iter()
            .filter_map(|(id, body)| {
                if conversation.is_some() && related != Some(id.as_str()) {
                    return None;
                }
                if !body.contains("**Status:** Ready") && !body.contains("**Status:** Implementing")
                {
                    return None;
                }
                let approved = p
                    .state
                    .workflow
                    .approved_features
                    .get(id)
                    .is_some_and(|saved| *saved == workflow::feature_contract(body));
                let prepare_tasks = p
                    .state
                    .active_feature
                    .as_ref()
                    .is_some_and(|(active, _)| active == id)
                    && !has_current_task_batch(p);
                (!approved || prepare_tasks).then(|| Action {
                    id: id.clone(),
                    specification: body.clone(),
                    approved,
                    prepare_tasks,
                })
            })
            .collect()
    }

    pub(super) fn approve_and_prepare_feature(&mut self, id: &str) {
        self.approve_feature_action(id, true);
    }

    pub(super) fn approve_feature_only(&mut self, id: &str) {
        self.approve_feature_action(id, false);
    }

    fn approve_feature_action(&mut self, id: &str, prepare_tasks: bool) {
        let Some(action) = self
            .available_feature_actions(None)
            .into_iter()
            .find(|a| a.id == id)
        else {
            return;
        };
        let Screen::Connected(p) = &mut self.screen else {
            return;
        };
        if p.active_turn.is_some()
            || p.task_turns.keys().any(|key| {
                p.state
                    .items
                    .iter()
                    .chain(&p.state.resolved_items)
                    .any(|item| {
                        item.conversation_key() == key && item.feature_id.as_deref() == Some(id)
                    })
            })
        {
            self.toasts
                .warning("Wait for the current planning reply before approving this feature.");
            return;
        }
        let contract = workflow::feature_contract(&action.specification);
        if let Err(error) = workflow::approve_feature_if_current(
            &p.state.repo_root,
            &mut p.state.workflow,
            id,
            Some(&contract),
        ) {
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!("Cannot approve {id}: {error}"),
                None,
            )]);
            self.toasts.danger(format!("Cannot approve {id}: {error}"));
            return;
        }
        let message =
            format!("Approved {id} for implementation against the reviewed feature contract.");
        p.activity.pending.push(message.clone());
        p.remember_chat(vec![ChatMessage::new(ChatRole::System, &message, None)]);
        // Put the application receipt beside requests in resolved conversations too.
        let keys = p
            .state
            .items
            .iter()
            .chain(&p.state.resolved_items)
            .filter(|item| item.feature_id.as_deref() == Some(id))
            .map(|item| item.conversation_key().to_owned())
            .collect::<Vec<_>>();
        for key in keys {
            if p.task_chats.messages.contains_key(&key) {
                p.task_chats.remember_response(
                    &p.chat_slug,
                    &key,
                    vec![ChatMessage::new(
                        ChatRole::System,
                        &message,
                        Some(key.clone()),
                    )],
                );
            }
        }
        self.toasts.success(&message);
        if !prepare_tasks || !action.prepare_tasks {
            return;
        }
        if !p.active_implementations.is_empty() {
            p.remember_chat(vec![ChatMessage::new(ChatRole::System,
                format!("{id} is approved. Use Prepare tasks for {id} after the current implementation workers finish."), None)]);
            return;
        }
        if p.state.workflow.ready(p.state.planning_contract()) {
            self.start_turn_with_purpose(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
            );
        } else {
            self.pending_feature_generation =
                Some((p.state.repo_root.clone(), id.into(), contract));
            self.start_turn_with_purpose(&format!(
                "Prepare the task-generation review for approved feature {id}. Its current specification is approved; do not ask for approval again or change its contract. Read the current feature and settled decisions, and return a complete interview brief naming {id}, ready_for_tasks=true when no blocking questions remain. Do not generate stories in this review turn. The application will generate them after a successful current review. If blocked, record the concrete blocking item."
            ), TurnPurpose::ReviewForGeneration);
            if !matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some()) {
                self.pending_feature_generation = None;
            }
        }
    }

    /// Only the completion of our Main Chat review can consume this continuation.
    /// A failed/cancelled review or changed contract never triggers generation.
    pub(super) fn continue_feature_generation(&mut self, applied: bool) {
        let Some((repo, id, contract)) = self.pending_feature_generation.take() else {
            return;
        };
        let Screen::Connected(p) = &mut self.screen else {
            return;
        };
        if p.state.repo_root != repo {
            return;
        }
        let current = p
            .state
            .active_feature
            .as_ref()
            .is_some_and(|(active, text)| {
                active == &id && workflow::feature_contract(text) == contract
            });
        if applied
            && current
            && p.state.workflow.ready(p.state.planning_contract())
            && !has_current_task_batch(p)
            && p.active_implementations.is_empty()
            && workflow::feature_approved(&repo, &p.state.workflow, &id)
        {
            self.start_turn_with_purpose(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
            );
        } else {
            let reason = if !current {
                "The feature contract or planning focus changed; review the current feature before continuing."
            } else {
                "The planning review did not produce a current, ready task plan. Resolve the reported issue, then use Prepare tasks to retry; the existing feature approval is retained."
            };
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!("Task preparation for {id} paused. {reason}"),
                None,
            )]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::board_tests::{click_text, fixture, frame, text_position};
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Harness {
        replies: Mutex<std::collections::VecDeque<String>>,
        calls: Arc<Mutex<Vec<String>>>,
    }
    impl crate::harness::AiHarness for Harness {
        fn label(&self) -> String {
            "approval fixture".into()
        }
        fn check_available(&self) -> Result<String, crate::error::AppError> {
            Ok(self.label())
        }
        fn execute(
            &self,
            req: &crate::harness::PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
            self.calls.lock().unwrap().push(req.prompt_body.clone());
            let text = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected model call");
            Ok(crate::harness::HarnessOutcome {
                final_text: text,
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
    fn harness(
        replies: Vec<String>,
    ) -> (Box<dyn crate::harness::AiHarness>, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Box::new(Harness {
                replies: Mutex::new(replies.into()),
                calls: calls.clone(),
            }),
            calls,
        )
    }
    fn setup() -> (PacketApp, std::path::PathBuf, serde_json::Value) {
        let mut app = fixture();
        let root = std::env::temp_dir().join(format!(
            "packet-approval-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.test"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let mut state = crate::core::state::PlannerState::load(&root).unwrap();
        state.bootstrap_missing().unwrap();
        let dir = root.join(".kool-ade-packet/planning/changes/CHG-004-saved-searches");
        std::fs::create_dir_all(&dir).unwrap();
        let mut spec = "# CHG-004: Saved searches\n\n**Status:** Ready\n".to_string();
        for heading in [
            "Intent",
            "Current Behavior",
            "Desired Behavior",
            "Scope",
            "Affected Product Areas",
            "Requirements",
            "Decisions and Assumptions",
            "Acceptance Criteria",
        ] {
            spec.push_str(&format!(
                "\n## {heading}\n\nPersist saved searches and restore them after restarting.\n"
            ));
        }
        spec = spec.replace(
            "## Affected Product Areas\n",
            "## Affected Product Areas\n\n`product:current-capabilities`\n",
        );
        std::fs::write(dir.join("specification.md"), spec).unwrap();
        let mut review: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/interview-ready.json"))
                .unwrap();
        review["updated_specification"] = serde_json::Value::Null;
        review["document_updates"] = serde_json::Value::Null;
        review["interview"]["feature_name"] = "Saved searches (CHG-004)".into();
        state.workflow.brief = Some(serde_json::from_value(review["interview"].clone()).unwrap());
        state.workflow.reviewed_specification = Some("Previous CHG-003 specification".into());
        crate::artifacts::task_docs::save_workflow(&root, &state.workflow).unwrap();
        let mut item = OpenItem::new(
            "CLR-026".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Branch retention choice".into(),
            "Settled".into(),
        );
        item.feature_id = Some("CHG-004".into());
        item.status = crate::domain::ItemStatus::Resolved;
        std::fs::write(
            root.join(".kool-ade-packet/planning/resolved-items.json"),
            serde_json::to_vec(&vec![item]).unwrap(),
        )
        .unwrap();
        crate::core::gitops::commit(
            &root,
            "fixture",
            &[
                ".kool-ade-packet/planning".into(),
                ".kool-ade-packet/config".into(),
            ],
        )
        .unwrap();
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        p.state = crate::core::state::PlannerState::load(&root).unwrap();
        p.task_documents.clear();
        p.implementation_states.clear();
        p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
        p.task_chats.remember_response(
            &p.chat_slug,
            "CLR-026",
            vec![ChatMessage::new(
                ChatRole::Agent,
                "Click Approve feature for implementation for CHG-004.",
                None,
            )],
        );
        (app, root, review)
    }
    fn finish(app: &mut PacketApp) -> bool {
        let Screen::Connected(p) = &app.screen else {
            panic!()
        };
        let started = Instant::now();
        let outcome = loop {
            assert!(started.elapsed() < Duration::from_secs(15));
            if let Some(TurnEvt::Done(outcome)) = p
                .active_turn
                .as_ref()
                .expect("turn started")
                .poll(Duration::from_millis(20))
            {
                break *outcome;
            }
        };
        let applied = matches!(&outcome, TurnOutcome::Applied { .. });
        let Screen::Connected(mut p) = std::mem::replace(&mut app.screen, Screen::Welcome) else {
            panic!()
        };
        let _ = app.adopt_turn(&mut p, outcome);
        app.screen = Screen::Connected(p);
        applied
    }

    #[test]
    fn approval_click_refreshes_stale_review_and_generates_stories_without_second_approval() {
        let _shield = crate::core::gitops::test_support::shield("feature-approval");
        let (mut app, root, review) = setup();
        assert!(
            app.task_offer().is_none(),
            "reproduce stale review hiding the old offer"
        );
        let (h, calls) = harness(vec![review.to_string()]);
        app.task_harness = Some(h);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Approve CHG-004 and prepare tasks").is_some());
        click_text(&mut app, &ctx, "Approve CHG-004 and prepare tasks");
        assert!(app.feature_approved("CHG-004"));
        assert!(matches!(&app.screen, Screen::Connected(project) if !project.queue.auto_publish));
        assert!(
            app.task_messages("CLR-026")
                .last()
                .unwrap()
                .text
                .contains("Approved CHG-004")
        );
        let applied = finish(&mut app);
        assert!(applied, "{:?}", app.chat_messages());
        assert!(calls.lock().unwrap()[0].contains("do not ask for approval again"));
        let (h, generated) = harness(vec![
            include_str!("../../tests/fixtures/task-outline.json").into(),
            include_str!("../../tests/fixtures/task-story-1.json").into(),
            include_str!("../../tests/fixtures/task-story-2.json").into(),
        ]);
        app.task_harness = Some(h);
        app.continue_feature_generation(applied);
        assert!(finish(&mut app), "{:?}", app.chat_messages());
        assert_eq!(generated.lock().unwrap().len(), 3);
        let Screen::Connected(p) = &app.screen else {
            panic!()
        };
        assert!(has_current_task_batch(p));
        assert!(
            p.task_documents
                .iter()
                .any(|doc| doc.text.contains("Feature ID: CHG-004"))
        );
        assert!(app.feature_actions(Some("CLR-026")).is_empty());
        assert!(app.implementation_offer());
        assert!(matches!(&app.screen, Screen::Connected(project) if !project.queue.auto_publish));
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolved_conversation_has_action_and_failed_review_retains_approval_for_retry() {
        let _shield = crate::core::gitops::test_support::shield("feature-approval-retry");
        let (mut app, root, _) = setup();
        let (h, _) = harness(vec!["malformed response".into()]);
        app.task_harness = Some(h);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "Open conversation");
        click_text(&mut app, &ctx, "Approve CHG-004 and prepare tasks");
        let applied = finish(&mut app);
        assert!(!applied);
        app.continue_feature_generation(applied);
        assert!(app.feature_approved("CHG-004"));
        assert!(app.pending_feature_generation.is_none());
        assert_eq!(
            app.feature_actions(Some("CLR-026"))[0].label(),
            "Prepare tasks for CHG-004"
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Prepare tasks for CHG-004").is_some());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_contract_stops_authorized_continuation_and_manager_uses_current_state() {
        let _shield = crate::core::gitops::test_support::shield("feature-approval-continuation");
        let (mut app, root, review) = setup();
        let (h, _) = harness(vec![review.to_string()]);
        app.task_harness = Some(h);
        app.approve_and_prepare_feature("CHG-004");
        assert!(finish(&mut app));
        assert!(
            !app.chat_messages()
                .last()
                .unwrap()
                .text
                .contains("Would you like to proceed")
        );
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        let prompt = crate::app::manager::Manager::prompt_body(p, &[]);
        assert!(prompt.contains("CHG-004: approved for current contract; do not ask again"));
        assert!(prompt.contains("Persist saved searches and restore them after restarting."));
        let path =
            root.join(".kool-ade-packet/planning/changes/CHG-004-saved-searches/specification.md");
        let changed = std::fs::read_to_string(&path)
            .unwrap()
            .replace("Persist saved searches", "Publish saved searches");
        std::fs::write(&path, changed).unwrap();
        p.state = crate::core::state::PlannerState::load(&root).unwrap();
        app.continue_feature_generation(true);
        let Screen::Connected(p) = &app.screen else {
            panic!()
        };
        assert!(p.active_turn.is_none());
        assert!(app.pending_feature_generation.is_none());
        assert!(
            app.chat_messages()
                .last()
                .unwrap()
                .text
                .contains("contract or planning focus changed")
        );
        assert_eq!(
            app.feature_actions(None)[0].label(),
            "Approve CHG-004 and prepare tasks"
        );
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_display_cannot_approve_changed_contract_or_overwrite_other_approval() {
        let _shield = crate::core::gitops::test_support::shield("feature-approval-drift");
        let (mut app, root, _) = setup();
        let path =
            root.join(".kool-ade-packet/planning/changes/CHG-004-saved-searches/specification.md");
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            original.replace("Persist saved searches", "Publish saved searches"),
        )
        .unwrap();
        app.approve_and_prepare_feature("CHG-004");
        assert!(!app.feature_approved("CHG-004"));
        assert!(
            app.chat_messages()
                .last()
                .unwrap()
                .text
                .contains("changed since it was displayed")
        );
        let Screen::Connected(p) = &app.screen else {
            panic!()
        };
        assert!(p.active_turn.is_none());
        std::fs::write(&path, original).unwrap();
        let mut saved = crate::artifacts::task_docs::load_workflow(&root).unwrap();
        saved
            .approved_features
            .insert("CHG-009".into(), "Other contract".into());
        crate::artifacts::task_docs::save_workflow(&root, &saved).unwrap();
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        workflow::approve_feature_if_current(
            &root,
            &mut p.state.workflow,
            "CHG-004",
            Some(&workflow::feature_contract(
                &p.state.active_feature.as_ref().unwrap().1,
            )),
        )
        .unwrap();
        assert_eq!(
            p.state.workflow.approved_features["CHG-009"],
            "Other contract"
        );
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
