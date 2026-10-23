pub(super) struct StartOptions {
    pub(super) manual: bool,
    pub(super) capabilities: crate::harness::runtime_capabilities::RuntimeCapabilities,
    pub(super) stale_session: Option<String>,
    pub(super) allow_offline: bool,
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
            stale_session: None,
            allow_offline: false,
            lifecycle_pr_publication: false,
            lifecycle_gate_refresh: false,
        }
    }
}
