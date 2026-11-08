use super::{ComputerIdentityV1, ComputerOperationV1, ComputerSessionClassV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Minimum disposable guest resource budget used by v1 host readiness checks.
/// Launchers must recheck the exact task policy budget before starting a VM.
pub const MINIMUM_DISPOSABLE_VM_BUDGET_V1: ComputerResourceBudgetV1 = ComputerResourceBudgetV1 {
    cpu_millis: 2_000,
    memory_bytes: 1_073_741_824,
    disk_bytes: 2_147_483_648,
    time_limit_ms: 600_000,
    action_limit: 100,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComputerCapabilityV1 {
    OfflineDisposableReadOnly,
    ApprovedGuiTask,
    InternetResearch,
    PersistentPersonal,
    HighImpactExternalAction,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComputerNetworkProfileV1 {
    Disabled,
    ApprovedInternet,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerResourceBudgetV1 {
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub time_limit_ms: u64,
    pub action_limit: u32,
}

impl ComputerResourceBudgetV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(100..=64_000).contains(&self.cpu_millis) {
            return Err("CPU quota is outside bounds");
        }
        if !(256 * 1024 * 1024..=128 * 1024 * 1024 * 1024).contains(&self.memory_bytes) {
            return Err("memory quota is outside bounds");
        }
        if !(256 * 1024 * 1024..=512 * 1024 * 1024 * 1024).contains(&self.disk_bytes) {
            return Err("disk quota is outside bounds");
        }
        if !(1_000..=24 * 60 * 60 * 1_000).contains(&self.time_limit_ms) {
            return Err("time quota is outside bounds");
        }
        if !(1..=100_000).contains(&self.action_limit) {
            return Err("action quota is outside bounds");
        }
        Ok(())
    }

    fn restricted_by(&self, ceiling: &Self) -> Self {
        Self {
            cpu_millis: self.cpu_millis.min(ceiling.cpu_millis),
            memory_bytes: self.memory_bytes.min(ceiling.memory_bytes),
            disk_bytes: self.disk_bytes.min(ceiling.disk_bytes),
            time_limit_ms: self.time_limit_ms.min(ceiling.time_limit_ms),
            action_limit: self.action_limit.min(ceiling.action_limit),
        }
    }
}

/// Immutable-by-interface grant produced only by the controller authorization broker.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerPolicyV1 {
    pub schema_version: u16,
    pub identity: ComputerIdentityV1,
    pub session_class: ComputerSessionClassV1,
    pub capability: ComputerCapabilityV1,
    pub network_profile: ComputerNetworkProfileV1,
    pub allowed_apps: BTreeSet<String>,
    pub authorized_operations: BTreeSet<ComputerOperationV1>,
    pub approval_generation: u64,
    pub resource_budget: ComputerResourceBudgetV1,
}

impl ComputerPolicyV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported policy version");
        }
        if self.capability == ComputerCapabilityV1::HighImpactExternalAction {
            return Err("high-impact actions are unsupported until exact transaction grants exist");
        }
        if self.session_class == ComputerSessionClassV1::PersonalPersistent {
            return Err("persistent sessions are unsupported until encrypted storage grants exist");
        }
        self.identity.validate()?;
        self.resource_budget.validate()?;
        if self.approval_generation == 0 {
            return Err("approval generation must be positive");
        }
        if self.allowed_apps.len() > 64 || self.allowed_apps.iter().any(|app| !valid_app_id(app)) {
            return Err("allowed app identifiers are invalid");
        }
        if self.capability == ComputerCapabilityV1::OfflineDisposableReadOnly
            && self.authorized_operations.iter().any(|operation| {
                !matches!(
                    operation,
                    ComputerOperationV1::ObserveScreen
                        | ComputerOperationV1::ObserveWindows
                        | ComputerOperationV1::ReadAccessibility
                        | ComputerOperationV1::WaitFor
                        | ComputerOperationV1::QueryState
                )
            })
        {
            return Err("read-only capability includes an input operation");
        }
        if self.network_profile == ComputerNetworkProfileV1::ApprovedInternet
            && self.capability < ComputerCapabilityV1::InternetResearch
        {
            return Err("network profile exceeds the capability grant");
        }
        if self.capability >= ComputerCapabilityV1::PersistentPersonal
            && self.session_class != ComputerSessionClassV1::PersonalPersistent
        {
            return Err("persistent capability requires a personal session class");
        }
        if self.session_class == ComputerSessionClassV1::Disposable
            && self.capability >= ComputerCapabilityV1::PersistentPersonal
        {
            return Err("disposable sessions cannot have persistent capabilities");
        }
        Ok(())
    }

    /// Intersects authorities and lowers every quota. This operation cannot promote a grant.
    pub fn restrict_to(&self, ceiling: &Self) -> Result<Self, &'static str> {
        self.validate()?;
        ceiling.validate()?;
        if self.identity != ceiling.identity || self.session_class != ceiling.session_class {
            return Err("policy restriction identity or session class mismatch");
        }
        let narrowed = Self {
            schema_version: 1,
            identity: self.identity.clone(),
            session_class: self.session_class,
            capability: self.capability.min(ceiling.capability),
            network_profile: self.network_profile.min(ceiling.network_profile),
            allowed_apps: self
                .allowed_apps
                .intersection(&ceiling.allowed_apps)
                .cloned()
                .collect(),
            authorized_operations: self
                .authorized_operations
                .intersection(&ceiling.authorized_operations)
                .copied()
                .collect(),
            approval_generation: self.approval_generation,
            resource_budget: self.resource_budget.restricted_by(&ceiling.resource_budget),
        };
        if self.approval_generation != ceiling.approval_generation {
            return Err("policy restriction approval generation mismatch");
        }
        narrowed.validate()?;
        Ok(narrowed)
    }
}

fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}
