use super::super::*;
use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};
use std::time::{Duration, Instant};

struct ResponseHarness(String);

impl AiHarness for ResponseHarness {
    fn label(&self) -> String {
        "deterministic Evolve task-generation fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(&self, _: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        Ok(HarnessOutcome {
            final_text: self.0.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn response_harness(response: String) -> Box<dyn AiHarness> {
    Box::new(ResponseHarness(response))
}

pub(super) fn finish_main_turn(app: &mut crate::app::root::KooladeApp) -> bool {
    let started = Instant::now();
    let outcome = loop {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "turn finishes in budget"
        );
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        if let Some(crate::core::turn::TurnEvt::Done(outcome)) = project
            .active_turn
            .as_ref()
            .expect("a planning turn started")
            .poll(Duration::from_millis(20))
        {
            break *outcome;
        }
    };
    let applied = matches!(&outcome, crate::core::turn::TurnOutcome::Applied { .. });
    let screen = std::mem::replace(&mut app.screen, Screen::Welcome);
    let Screen::Connected(mut project) = screen else {
        panic!("project remains connected")
    };
    let _ = app.adopt_turn(&mut project, outcome);
    app.screen = Screen::Connected(project);
    applied
}
