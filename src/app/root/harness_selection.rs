use crate::harness::{AiHarness, PiHarness};

pub(crate) fn configured_harness(
    override_harness: &mut Option<Box<dyn AiHarness>>,
) -> Box<dyn AiHarness> {
    if let Some(harness) = override_harness.take() {
        return harness;
    }
    match crate::persistence::harness_settings::load()
        .0
        .default_harness
        .as_deref()
    {
        None | Some("pi") => Box::new(PiHarness),
        Some(id) => Box::new(UnavailableHarness(id.to_owned())),
    }
}

struct UnavailableHarness(String);

impl AiHarness for UnavailableHarness {
    fn label(&self) -> String {
        format!("{} (unavailable)", self.0)
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Err(self.error())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Err(self.error())
    }
}

impl UnavailableHarness {
    fn error(&self) -> crate::error::AppError {
        crate::error::AppError::Other(format!(
            "The saved default coding harness '{}' is unavailable in this Kool.ad/e build. Open Coding tool configuration to choose an available harness.",
            self.0
        ))
    }
}

#[cfg(test)]
#[path = "harness_selection/tests.rs"]
mod tests;
