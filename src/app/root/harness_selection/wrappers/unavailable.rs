use super::*;

pub(in crate::app::root::harness_selection) struct UnavailableHarness {
    pub(super) message: String,
    pub(super) saved_route: bool,
    pub(super) refresh_binding: Option<crate::harness::CapabilityRefreshBindingV1>,
}

impl AiHarness for UnavailableHarness {
    fn capability_refresh_binding(&self) -> Option<crate::harness::CapabilityRefreshBindingV1> {
        self.refresh_binding.clone()
    }

    fn label(&self) -> String {
        format!("{} (unavailable)", self.message)
    }

    fn route_blocker(&self) -> Option<String> {
        Some(self.error().to_string())
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Err(self.error())
    }

    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Err(self.error())
    }
}

impl UnavailableHarness {
    fn error(&self) -> crate::error::AppError {
        crate::error::AppError::Other(if self.saved_route {
            format!(
                "The saved coding harness route '{}' is unavailable in this Kool.ad/e build. Open Coding tool configuration to choose an available harness.",
                self.message
            )
        } else {
            self.message.clone()
        })
    }

    pub(in crate::app::root::harness_selection) fn guidance(message: String) -> Self {
        Self {
            message,
            saved_route: false,
            refresh_binding: None,
        }
    }

    pub(in crate::app::root::harness_selection) fn refreshing(
        message: String,
        binding: crate::harness::CapabilityRefreshBindingV1,
    ) -> Self {
        Self {
            message,
            saved_route: false,
            refresh_binding: Some(binding),
        }
    }

    pub(in crate::app::root::harness_selection) fn unknown_saved_route(route_id: String) -> Self {
        Self {
            message: route_id,
            saved_route: true,
            refresh_binding: None,
        }
    }
}
