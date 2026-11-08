use super::*;
use crate::core::planning::{
    MATERIAL_AXES, QuestionCategory, QuestionOption, QuestionPriority, QuestionProposal,
    QuestionStore, ReadinessChecklistV1, propose_questions,
};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

struct ScriptedEvolveHarness {
    output: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl crate::harness::AiHarness for ScriptedEvolveHarness {
    fn label(&self) -> String {
        "Evolve clarification fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.requests
            .lock()
            .unwrap()
            .push(request.prompt_body.clone());
        thread::sleep(Duration::from_millis(250));
        Ok(crate::harness::HarnessOutcome {
            final_text: self.output.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[cfg(unix)]
#[path = "clarification/pursuit_readiness.rs"]
mod pursuit_readiness;

#[cfg(unix)]
fn readiness_diagnostics(
    project: &crate::app::session::Project,
    feature_id: &str,
    feature_uid: &str,
    question_uid: &str,
    manager_requests: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    planner_requests: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> String {
    let questions = QuestionStore::new(&project.state.planning_store);
    let question_status = questions
        .load(question_uid)
        .ok()
        .map(|(question, _)| (question.status, question.revision));
    let readiness = questions
        .load_readiness(feature_uid)
        .ok()
        .flatten()
        .map(|(record, _)| (record.plan_revision, record.ready));
    let manager_requests = manager_requests
        .lock()
        .map(|items| items.len())
        .unwrap_or(0);
    let planner_request_prefixes = planner_requests
        .lock()
        .map(|items| {
            items
                .iter()
                .map(|prompt| {
                    prompt
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(120)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let planner_request_count = planner_request_prefixes.len();
    let feature_updated = project.state.active_features.iter().any(|(id, body)| {
        id == feature_id && body.contains("The user chose workspace-specific retention periods.")
    });
    let recent_chat = project
        .chat
        .iter()
        .rev()
        .take(6)
        .map(|message| message.text.replace('\n', " "))
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "active_turn={}, planner_running={}, pending_planner={}, active_work={}, question={question_status:?}, readiness={readiness:?}, manager_requests={manager_requests}, planner_requests={planner_request_count} ({planner_request_prefixes:?}), feature_updated={feature_updated}, recent_chat={recent_chat}",
        project.active_turn.is_some(),
        project.activity.clarification_planner.is_some(),
        project.activity.pending_clarification_planner.is_some(),
        project.active_planning_work.is_some(),
    )
}

#[cfg(unix)]
fn wait_for_readiness(
    app: &mut crate::app::root::KooladeApp,
    planner_output: &str,
    planner_requests: &Arc<Mutex<Vec<String>>>,
    manager_requests: &Arc<Mutex<Vec<String>>>,
    feature_id: &str,
    feature_uid: &str,
    question_uid: &str,
) {
    let context = egui::Context::default();
    // Manager and Planner replies cross asynchronous app ticks. Allow the
    // production-boundary fixture to finish while the parallel full suite is
    // exercising other lifecycle and execution tests on the same host.
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    loop {
        app.task_harness = Some(Box::new(ScriptedEvolveHarness {
            output: planner_output.to_owned(),
            requests: Arc::clone(planner_requests),
        }));
        if let crate::app::root::Screen::Connected(project) = &mut app.screen {
            // This test owns the one-shot harness slot for its Evolve follow-up.
            // Keep unrelated automatic Manager patrols from consuming a Planner
            // response while the asynchronous turn/planner handoff is in flight.
            project.activity.last_update = Some(std::time::Instant::now());
        }
        app.tick(0.016, &context);
        let completed = match &app.screen {
            crate::app::root::Screen::Connected(project) => {
                let questions = QuestionStore::new(&project.state.planning_store);
                questions.load_readiness(feature_uid).is_ok_and(|record| {
                    record.is_some_and(|(readiness, _)| {
                        readiness.plan_revision == 2 && readiness.ready
                    })
                }) && questions.load(question_uid).is_ok_and(|(saved, _)| {
                    saved.status == crate::core::planning::QuestionStatus::Superseded
                })
            }
            crate::app::root::Screen::Welcome => false,
        };
        if completed {
            return;
        }
        if std::time::Instant::now() >= deadline {
            let diagnostics = match &app.screen {
                crate::app::root::Screen::Connected(project) => readiness_diagnostics(
                    project,
                    feature_id,
                    feature_uid,
                    question_uid,
                    manager_requests,
                    planner_requests,
                ),
                crate::app::root::Screen::Welcome => "screen disconnected".into(),
            };
            panic!(
                "Evolve clarification did not persist Manager update and Planner readiness: {diagnostics}"
            );
        }
        thread::sleep(Duration::from_millis(16));
    }
}
