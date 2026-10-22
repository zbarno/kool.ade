use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn take_over_stale_task_claim(&mut self, ticket: String) {
        let session = match &self.screen {
            Screen::Connected(project) => project
                .queue
                .stale_claim
                .as_ref()
                .filter(|(claimed_ticket, _)| claimed_ticket == &ticket)
                .map(|(_, record)| record.session_id.clone()),
            Screen::Welcome => None,
        };
        let Some(session) = session else { return };
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(
            ticket,
            true,
            capabilities,
            Some(session),
            false,
            false,
        );
    }

    pub(in crate::app::root) fn start_implementation(&mut self, ticket: String, manual: bool) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_capabilities(ticket, manual, capabilities);
    }

    /// Only an explicit Task Details action may opt this single run out of
    /// cross-clone coordination. Automatic starts cannot reach this entry point.
    pub(in crate::app::root) fn start_without_shared_coordination(&mut self, ticket: String) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(ticket, true, capabilities, None, true, false);
    }

    pub(in crate::app::root) fn start_implementation_with_capabilities(
        &mut self,
        ticket: String,
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) {
        self.start_implementation_with_claim_mode(ticket, manual, capabilities, None, false, false);
    }

    pub(in crate::app::root) fn start_lifecycle_pr_publication(&mut self, ticket: String) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(ticket, true, capabilities, None, false, true);
    }
}
