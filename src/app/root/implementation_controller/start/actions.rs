use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn start_implementation(&mut self, ticket: String, manual: bool) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_capabilities(ticket, manual, capabilities);
    }

    /// Only an explicit Task Details action may opt this single run out of
    /// cross-clone coordination. Automatic starts cannot reach this entry point.
    pub(in crate::app::root) fn start_without_shared_coordination(&mut self, ticket: String) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(ticket, true, capabilities, None, true);
    }

    pub(in crate::app::root) fn start_implementation_with_capabilities(
        &mut self,
        ticket: String,
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) {
        self.start_implementation_with_claim_mode(ticket, manual, capabilities, None, false);
    }
}
