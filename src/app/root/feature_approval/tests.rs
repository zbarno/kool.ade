use super::super::board_tests::{click_text, fixture, frame, text_position};
use super::*;
use sha2::Digest;
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
        let text = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
            let tail = &req.prompt_body[req.prompt_body.len().saturating_sub(1200)..];
            panic!(
                "unexpected model call in {:?}; prompt tail:\n{tail}",
                req.mode
            )
        });
        Ok(crate::harness::HarnessOutcome {
            final_text: text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
fn harness(replies: Vec<String>) -> (Box<dyn crate::harness::AiHarness>, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        Box::new(Harness {
            replies: Mutex::new(replies.into()),
            calls: calls.clone(),
        }),
        calls,
    )
}

#[path = "tests/support.rs"]
mod support;
use support::decomposer_graph;
pub(crate) use support::setup;
pub(crate) use support::{
    assert_current_test_plan_confirmation, record_test_plan_author, record_test_plan_review,
    remove_fixture_root,
};

fn finish(app: &mut KooladeApp) -> bool {
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    let started = Instant::now();
    let outcome = loop {
        // Keep the test bounded while allowing the worker to be scheduled
        // during parallel nextest runs on a loaded host.
        assert!(started.elapsed() < Duration::from_secs(60));
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
    let _ = app.adopt_turn(&mut p, outcome, None, None, false);
    app.screen = Screen::Connected(p);
    applied
}

#[path = "tests/approval_flow.rs"]
mod approval_flow;
#[path = "tests/contract.rs"]
mod contract;
#[path = "tests/live_generation.rs"]
mod live_generation;
#[path = "tests/plan_review.rs"]
mod plan_review;
#[path = "tests/readiness.rs"]
mod readiness;
#[path = "tests/review_recovery.rs"]
mod review_recovery;
