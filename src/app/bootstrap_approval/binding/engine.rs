use crate::execution::docker::engine::EngineDiscovery;

use super::MachineBinding;

mod current;
mod smoke;

pub(in crate::app::bootstrap_approval) struct VerifiedEngine {
    binding: MachineBinding,
    engine: EngineDiscovery,
    #[cfg(target_os = "linux")]
    endpoint_fingerprint: String,
}

impl VerifiedEngine {
    pub(in crate::app::bootstrap_approval) fn binding(&self) -> &MachineBinding {
        &self.binding
    }

    pub(in crate::app::bootstrap_approval) fn into_binding(self) -> MachineBinding {
        self.binding
    }
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;
