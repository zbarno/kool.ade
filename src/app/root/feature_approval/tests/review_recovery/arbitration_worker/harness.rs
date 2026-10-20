use std::sync::{Arc, Mutex};

struct ArbitrationHarness {
    replies: Arc<Mutex<std::collections::VecDeque<String>>>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl crate::harness::AiHarness for ArbitrationHarness {
    fn label(&self) -> String {
        "arbitration integration fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.prompts
            .lock()
            .unwrap()
            .push(request.prompt_body.clone());
        let response = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("the Manager and arbitration each receive a fixture response");
        Ok(crate::harness::HarnessOutcome {
            final_text: response,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

pub(super) fn harness(
    replies: Arc<Mutex<std::collections::VecDeque<String>>>,
    prompts: Arc<Mutex<Vec<String>>>,
) -> Box<dyn crate::harness::AiHarness> {
    Box::new(ArbitrationHarness { replies, prompts })
}
