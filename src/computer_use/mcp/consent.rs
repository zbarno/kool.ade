//! In-memory, app-mediated consent requests for Computer Use session grants.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use super::ComputerUseCallerV1;

const MAX_PENDING_REQUESTS: usize = 16;
const CONSENT_TIMEOUT: Duration = Duration::from_secs(60);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);
const GRANTABLE_OPERATIONS: [crate::computer_use::ComputerOperationV1; 9] = [
    crate::computer_use::ComputerOperationV1::ObserveScreen,
    crate::computer_use::ComputerOperationV1::ObserveWindows,
    crate::computer_use::ComputerOperationV1::ReadAccessibility,
    crate::computer_use::ComputerOperationV1::MouseInput,
    crate::computer_use::ComputerOperationV1::KeyboardInput,
    crate::computer_use::ComputerOperationV1::LaunchAllowedApp,
    crate::computer_use::ComputerOperationV1::ManageApp,
    crate::computer_use::ComputerOperationV1::WaitFor,
    crate::computer_use::ComputerOperationV1::QueryState,
];
const GRANTABLE_APPS: [&str; 5] = [
    "chromium",
    "chromium-research",
    "file-manager",
    "text-editor",
    "test-gui-fixture",
];

/// App-selected target and least-privilege policy shown before the operator
/// approves a session. This data never comes from provider tool arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerUseConsentScopeV1 {
    pub machine_id: String,
    pub approved_destination: Option<String>,
    pub allowed_operations: Vec<crate::computer_use::ComputerOperationV1>,
    pub allowed_apps: Vec<String>,
    pub network_profile: crate::computer_use::ComputerNetworkProfileV1,
    /// Research networking is a separate, human-selected consent option. The
    /// provider cannot request or promote this profile through its tool input.
    pub internet_research_available: bool,
    pub resource_budget: crate::computer_use::ComputerResourceBudgetV1,
}

impl ComputerUseConsentScopeV1 {
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        self.validate_for_role("implementer/v1")
    }

    pub(crate) fn validate_for_role(&self, role_id: &str) -> Result<(), &'static str> {
        valid_scope(self, role_id)
            .then_some(())
            .ok_or("Computer Use consent scope is invalid")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerUseConsentRequestV1 {
    pub request_id: String,
    pub identity: crate::computer_use::ComputerIdentityV1,
    pub role_id: String,
    pub purpose: String,
    pub machine_id: String,
    pub approved_destination: Option<String>,
    pub allowed_operations: Vec<crate::computer_use::ComputerOperationV1>,
    pub allowed_apps: Vec<String>,
    pub network_profile: crate::computer_use::ComputerNetworkProfileV1,
    pub available_network_profiles: Vec<crate::computer_use::ComputerNetworkProfileV1>,
    pub resource_budget: crate::computer_use::ComputerResourceBudgetV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputerUseConsentDecisionV1 {
    Approved(Box<ComputerUseConsentGrantV1>),
    Denied,
    Cancelled,
    TimedOut,
}

/// Immutable-by-convention result of the explicit UI approval. The session
/// issuer must make its signed policy no broader than this exact scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerUseConsentGrantV1 {
    pub(crate) request: ComputerUseConsentRequestV1,
    pub(crate) approved_at_unix_ms: u64,
    pub(crate) expires_at_unix_ms: u64,
}

struct PendingRequest {
    view: ComputerUseConsentRequestV1,
    reply: mpsc::SyncSender<ComputerUseConsentDecisionV1>,
}

#[derive(Default)]
struct State {
    pending: BTreeMap<String, PendingRequest>,
}

/// Process-local consent handoff between provider bridge threads and the app UI.
/// Provider calls can enqueue requests but only an explicit UI decision can
/// approve one. Pending state is intentionally not persisted across launches.
#[derive(Clone, Default)]
pub struct ComputerUseConsentBrokerV1 {
    state: Arc<Mutex<State>>,
}

#[cfg(test)]
#[path = "consent/tests.rs"]
mod tests;

impl ComputerUseConsentBrokerV1 {
    pub fn pending(&self) -> Vec<ComputerUseConsentRequestV1> {
        self.state
            .lock()
            .map(|state| {
                state
                    .pending
                    .values()
                    .map(|pending| pending.view.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn decide(&self, request_id: &str, approve: bool) -> Result<(), &'static str> {
        if approve {
            self.decide_with_network_profile(
                request_id,
                crate::computer_use::ComputerNetworkProfileV1::Disabled,
            )
        } else {
            self.finish_decision(request_id, None)
        }
    }

    pub fn decide_with_network_profile(
        &self,
        request_id: &str,
        network_profile: crate::computer_use::ComputerNetworkProfileV1,
    ) -> Result<(), &'static str> {
        self.finish_decision(request_id, Some(network_profile))
    }

    fn finish_decision(
        &self,
        request_id: &str,
        network_profile: Option<crate::computer_use::ComputerNetworkProfileV1>,
    ) -> Result<(), &'static str> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Computer Use consent state is unavailable")?;
        let pending_view = state
            .pending
            .get(request_id)
            .ok_or("Computer Use consent request is no longer pending")?;
        if network_profile.is_some_and(|profile| {
            !pending_view
                .view
                .available_network_profiles
                .contains(&profile)
        }) {
            return Err("selected network profile was not offered by the app");
        }
        let pending = state
            .pending
            .remove(request_id)
            .ok_or("Computer Use consent request is no longer pending")?;
        drop(state);
        let decision = if let Some(network_profile) = network_profile {
            let Some(approved_at_unix_ms) = unix_time_ms() else {
                let _ = pending.reply.send(ComputerUseConsentDecisionV1::Denied);
                return Err("system clock is unavailable");
            };
            let mut request = pending.view;
            request.network_profile = network_profile;
            if network_profile == crate::computer_use::ComputerNetworkProfileV1::InternetResearch
                && request.allowed_apps.iter().any(|app| app == "chromium")
            {
                request.allowed_apps.retain(|app| app != "chromium");
                request.allowed_apps.push("chromium-research".to_owned());
                request.allowed_apps.sort();
                request.allowed_apps.dedup();
            }
            ComputerUseConsentDecisionV1::Approved(Box::new(ComputerUseConsentGrantV1 {
                request,
                approved_at_unix_ms,
                expires_at_unix_ms: approved_at_unix_ms.saturating_add(
                    crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1.time_limit_ms,
                ),
            }))
        } else {
            ComputerUseConsentDecisionV1::Denied
        };
        pending
            .reply
            .send(decision)
            .map_err(|_| "Computer Use consent request has already ended")?;
        Ok(())
    }

    pub fn request(
        &self,
        caller: &ComputerUseCallerV1,
        purpose: &str,
        scope: ComputerUseConsentScopeV1,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> ComputerUseConsentDecisionV1 {
        if caller.identity.validate().is_err()
            || !super::is_supported_computer_use_role_v1(&caller.role_id)
            || purpose.trim().is_empty()
            || purpose.len() > super::MAX_COMPUTER_USE_PURPOSE_BYTES
            || scope.validate_for_role(&caller.role_id).is_err()
        {
            return ComputerUseConsentDecisionV1::Denied;
        }
        let request_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let (reply, decision) = mpsc::sync_channel(1);
        let pending = PendingRequest {
            view: ComputerUseConsentRequestV1 {
                request_id: request_id.clone(),
                identity: caller.identity.clone(),
                role_id: caller.role_id.clone(),
                purpose: purpose.trim().to_owned(),
                machine_id: scope.machine_id,
                approved_destination: scope.approved_destination,
                allowed_operations: scope.allowed_operations,
                allowed_apps: scope.allowed_apps,
                network_profile: scope.network_profile,
                available_network_profiles: if scope.internet_research_available {
                    vec![
                        crate::computer_use::ComputerNetworkProfileV1::Disabled,
                        crate::computer_use::ComputerNetworkProfileV1::InternetResearch,
                    ]
                } else {
                    vec![crate::computer_use::ComputerNetworkProfileV1::Disabled]
                },
                resource_budget: scope.resource_budget,
            },
            reply,
        };
        {
            let Ok(mut state) = self.state.lock() else {
                return ComputerUseConsentDecisionV1::Denied;
            };
            if state.pending.len() >= MAX_PENDING_REQUESTS {
                return ComputerUseConsentDecisionV1::Denied;
            }
            state.pending.insert(request_id.clone(), pending);
        }

        let started = Instant::now();
        let result = loop {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                break ComputerUseConsentDecisionV1::Cancelled;
            }
            if started.elapsed() >= CONSENT_TIMEOUT {
                break ComputerUseConsentDecisionV1::TimedOut;
            }
            match decision.recv_timeout(CANCEL_POLL_INTERVAL) {
                Ok(decision) => break decision,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break ComputerUseConsentDecisionV1::Denied;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        if let Ok(mut state) = self.state.lock() {
            state.pending.remove(&request_id);
        }
        result
    }
}

fn valid_scope(scope: &ComputerUseConsentScopeV1, role_id: &str) -> bool {
    let allowed_operations = scope
        .allowed_operations
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let allowed_apps = scope
        .allowed_apps
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let (operation_limit, app_limit) = if role_id == super::COMPUTER_USE_RUNTIME_QA_ROLE_V1 {
        (
            &super::RUNTIME_QA_COMPUTER_USE_OPERATIONS_V1[..],
            &[super::COMPUTER_USE_RUNTIME_QA_APP_ID_V1][..],
        )
    } else {
        (&GRANTABLE_OPERATIONS[..], &GRANTABLE_APPS[..])
    };
    uuid::Uuid::parse_str(&scope.machine_id)
        .is_ok_and(|machine| machine.to_string() == scope.machine_id)
        && !scope.allowed_operations.is_empty()
        && allowed_operations.len() == scope.allowed_operations.len()
        && scope
            .allowed_operations
            .iter()
            .all(|operation| operation_limit.contains(operation))
        && !scope.allowed_apps.is_empty()
        && allowed_apps.len() == scope.allowed_apps.len()
        && scope
            .allowed_apps
            .iter()
            .all(|app| app_limit.contains(&app.as_str()))
        && scope.network_profile == crate::computer_use::ComputerNetworkProfileV1::Disabled
        && scope.approved_destination.is_none()
        && scope.resource_budget.validate().is_ok()
        && resource_budget_within(
            &scope.resource_budget,
            &crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        )
}

fn resource_budget_within(
    actual: &crate::computer_use::ComputerResourceBudgetV1,
    maximum: &crate::computer_use::ComputerResourceBudgetV1,
) -> bool {
    actual.cpu_millis <= maximum.cpu_millis
        && actual.memory_bytes <= maximum.memory_bytes
        && actual.disk_bytes <= maximum.disk_bytes
        && actual.time_limit_ms <= maximum.time_limit_ms
        && actual.action_limit <= maximum.action_limit
}

fn unix_time_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| duration.as_millis().try_into().ok())
}
