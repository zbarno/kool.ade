use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn start_implementation(&mut self, ticket: String, manual: bool) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_capabilities(ticket, manual, capabilities);
    }

    pub(in crate::app::root) fn start_implementation_with_capabilities(
        &mut self,
        ticket: String,
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) {
        self.start_implementation_with_claim_mode(ticket, StartOptions::new(manual, capabilities));
    }

    pub(in crate::app::root) fn start_lifecycle_pr_publication(&mut self, ticket: String) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(
            ticket,
            StartOptions {
                lifecycle_pr_publication: true,
                ..StartOptions::new(true, capabilities)
            },
        );
    }

    pub(in crate::app::root) fn start_lifecycle_gate_refresh(&mut self, ticket: String) {
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities::detect();
        self.start_implementation_with_claim_mode(
            ticket,
            StartOptions {
                lifecycle_gate_refresh: true,
                ..StartOptions::new(true, capabilities)
            },
        );
    }
}
