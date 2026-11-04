pub(super) struct StartOptions {
    pub(super) manual: bool,
    pub(super) capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    pub(super) lifecycle_pr_publication: bool,
    pub(super) lifecycle_gate_refresh: bool,
}

impl StartOptions {
    pub(super) fn new(
        manual: bool,
        capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    ) -> Self {
        Self {
            manual,
            capabilities,
            lifecycle_pr_publication: false,
            lifecycle_gate_refresh: false,
        }
    }
}
