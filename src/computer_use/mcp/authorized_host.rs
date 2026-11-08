//! Registry-fenced MCP host: only session requests reach controller operations.

mod validation;
use validation::{
    blocked, denied, resource_budget_within, session_id, target_machine_id, unix_time_ms,
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use super::{
    ComputerUseCallerV1, ComputerUseMcpHostV1, ComputerUseMcpRequestV1 as Request,
    ComputerUseMcpResponseV1 as Response, ComputerUseMcpStatusV1 as Status,
    MAX_COMPUTER_USE_PURPOSE_BYTES, is_supported_computer_use_role_v1,
};
use crate::computer_use::{
    ComputerCapabilityV1, ComputerSessionClassV1,
    guest_protocol::{AuthorizedComputerUseSessionV1, ComputerUseAuthorityRegistryV1},
};

/// App controller operations. Session-scoped calls receive an already
/// identity, role, expiry, signature, lease, and revocation checked authority.
/// Implementations must use `AuthorizedComputerUseSessionV1::execute` for
/// guest actions so machine and lease bindings stay enforced at dispatch.
pub trait ComputerUseMcpOperationsV1: Send + Sync {
    fn capabilities(&self, caller: &ComputerUseCallerV1) -> Response;

    /// Resolve the target and bounded policy from app-owned runtime state
    /// before asking the user. Invalid or unavailable targets must fail closed.
    fn consent_scope(
        &self,
        caller: &ComputerUseCallerV1,
    ) -> Result<super::ComputerUseConsentScopeV1, Box<Response>>;

    /// Prepare the granted session after the host wrapper records explicit
    /// consent. Implementations cannot approve requests through this method.
    fn request_session(
        &self,
        caller: &ComputerUseCallerV1,
        grant: &super::ComputerUseConsentGrantV1,
        cancelled: &AtomicBool,
    ) -> Response;

    fn dispatch_authorized(
        &self,
        caller: &ComputerUseCallerV1,
        authority: Arc<AuthorizedComputerUseSessionV1>,
        request: Request,
        cancelled: &AtomicBool,
    ) -> Response;

    /// Release app-owned resources for a session. Implementations without a VM
    /// lifecycle may leave the default no-op; the wrapper still revokes authority.
    fn revoke_session(&self, _caller: &ComputerUseCallerV1, session_id: &str) -> Response {
        Response {
            schema_version: super::COMPUTER_USE_MCP_VERSION_V1,
            status: Status::Completed,
            summary: "Computer Use session cleanup completed.".into(),
            reason_code: None,
            session_id: Some(session_id.to_owned()),
            capabilities: Vec::new(),
            observation: None,
            action_outcome: None,
        }
    }
}

/// Concrete app-owned host wrapper. The controller supplies lifecycle work;
/// this layer fences each provider request against current registered authority.
pub struct RegistryBoundComputerUseMcpHostV1 {
    registry: Arc<ComputerUseAuthorityRegistryV1>,
    consent: super::ComputerUseConsentBrokerV1,
    operations: Arc<dyn ComputerUseMcpOperationsV1>,
    requesting_sessions: Arc<Mutex<HashSet<String>>>,
}

impl RegistryBoundComputerUseMcpHostV1 {
    pub fn new(
        registry: Arc<ComputerUseAuthorityRegistryV1>,
        consent: super::ComputerUseConsentBrokerV1,
        operations: Arc<dyn ComputerUseMcpOperationsV1>,
    ) -> Self {
        Self {
            registry,
            consent,
            operations,
            requesting_sessions: Arc::default(),
        }
    }

    fn reserve_session_request(&self, session_id: &str) -> Option<SessionRequestGuard> {
        let mut requesting = self.requesting_sessions.lock().ok()?;
        if !requesting.insert(session_id.to_owned()) {
            return None;
        }
        Some(SessionRequestGuard {
            session_id: session_id.to_owned(),
            requesting_sessions: self.requesting_sessions.clone(),
        })
    }

    fn handle_inner(
        &self,
        caller: &ComputerUseCallerV1,
        request: Request,
        cancelled: &AtomicBool,
    ) -> Response {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return denied("cancelled");
        }
        if caller.identity.validate().is_err()
            || !is_supported_computer_use_role_v1(&caller.role_id)
        {
            return denied("authorization_failed");
        }
        let response = match request {
            Request::Capabilities => self.operations.capabilities(caller),
            Request::RequestSession { purpose } => {
                if purpose.trim().is_empty() || purpose.len() > MAX_COMPUTER_USE_PURPOSE_BYTES {
                    return denied("invalid_request");
                }
                let Some(_request_guard) =
                    self.reserve_session_request(&caller.identity.session_id)
                else {
                    return denied("session_request_in_progress");
                };
                match self.registry.contains_session(&caller.identity.session_id) {
                    Ok(true) => return denied("session_already_registered"),
                    Ok(false) => {}
                    Err(_) => return denied("authorization_unavailable"),
                }
                let scope = match self.operations.consent_scope(caller) {
                    Ok(scope) => scope,
                    Err(response) => return *response,
                };
                if scope.validate().is_err() {
                    return blocked("computer_use_target_unavailable");
                }
                match self.consent.request(caller, &purpose, scope, cancelled) {
                    super::ComputerUseConsentDecisionV1::Approved(grant) => {
                        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            return denied("cancelled");
                        }
                        let response = self.operations.request_session(caller, &grant, cancelled);
                        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            let cleanup = self.cleanup_session(caller);
                            return if cleanup.status == Status::Completed {
                                denied("cancelled")
                            } else {
                                cleanup
                            };
                        }
                        if response.status != Status::Ready {
                            let cleanup = self.cleanup_session(caller);
                            if cleanup.status == Status::Completed {
                                response
                            } else {
                                cleanup
                            }
                        } else if self.validate_issued_grant(caller, &grant, &response) {
                            response
                        } else {
                            // A controller may have registered authority before
                            // returning a malformed or out-of-scope response.
                            // Revoke it before reporting failure so it cannot be
                            // reused by a later provider call.
                            let cleanup = self.cleanup_session(caller);
                            if cleanup.status == Status::Completed {
                                denied("issued_grant_exceeds_approval")
                            } else {
                                cleanup
                            }
                        }
                    }
                    super::ComputerUseConsentDecisionV1::Denied => denied("consent_denied"),
                    super::ComputerUseConsentDecisionV1::Cancelled => denied("cancelled"),
                    super::ComputerUseConsentDecisionV1::TimedOut => blocked("consent_timeout"),
                }
            }
            request => {
                let Some(session_id) = session_id(&request) else {
                    return denied("invalid_request");
                };
                // The app binds this invocation to one computer session. A
                // model cannot pick another user's or task's session by ID.
                if session_id != caller.identity.session_id {
                    return denied("authorization_failed");
                }
                let now = unix_time_ms();
                let Ok(authority) =
                    self.registry
                        .authorize(&caller.identity, caller.actor, &caller.role_id, now)
                else {
                    return denied("authorization_failed");
                };
                let response = self
                    .operations
                    .dispatch_authorized(caller, authority, request, cancelled);
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    let cleanup = self.cleanup_session(caller);
                    if cleanup.status == Status::Completed {
                        denied("cancelled")
                    } else {
                        cleanup
                    }
                } else {
                    response
                }
            }
        };
        if response.validate().is_err() {
            return denied("invalid_controller_response");
        }
        response
    }

    fn cleanup_session(&self, caller: &ComputerUseCallerV1) -> Response {
        let response = self
            .operations
            .revoke_session(caller, &caller.identity.session_id);
        // Authority revocation is required even if resource cleanup reports a
        // failure. This also preserves the registry fence for generic handlers.
        self.registry.revoke(&caller.identity.session_id).ok();
        response
    }

    fn validate_issued_grant(
        &self,
        caller: &ComputerUseCallerV1,
        grant: &super::ComputerUseConsentGrantV1,
        response: &Response,
    ) -> bool {
        let Some(session_id) = response.session_id.as_deref() else {
            return false;
        };
        let request = &grant.request;
        if request.identity != caller.identity
            || request.role_id != caller.role_id
            || grant.expires_at_unix_ms <= unix_time_ms()
            || session_id != caller.identity.session_id
        {
            return false;
        }
        let Ok(authority) = self.registry.authorize(
            &caller.identity,
            caller.actor,
            &caller.role_id,
            unix_time_ms(),
        ) else {
            return false;
        };
        let session = authority.session();
        let policy = &session.policy;
        policy.identity == request.identity
            && target_machine_id(session) == request.machine_id
            && request.approved_destination.is_none()
            && policy.session_class == ComputerSessionClassV1::Disposable
            && policy.capability == ComputerCapabilityV1::ApprovedGuiTask
            && policy.network_profile == request.network_profile
            && policy
                .authorized_operations
                .iter()
                .all(|operation| request.allowed_operations.contains(operation))
            && policy
                .allowed_apps
                .iter()
                .all(|app| request.allowed_apps.contains(app))
            && resource_budget_within(&policy.resource_budget, &request.resource_budget)
            && session.lease.expires_at_unix_ms <= grant.expires_at_unix_ms
            && session.session_started_at_unix_ms >= grant.approved_at_unix_ms
    }
}

struct SessionRequestGuard {
    session_id: String,
    requesting_sessions: Arc<Mutex<HashSet<String>>>,
}

impl Drop for SessionRequestGuard {
    fn drop(&mut self) {
        if let Ok(mut requesting) = self.requesting_sessions.lock() {
            requesting.remove(&self.session_id);
        }
    }
}

impl ComputerUseMcpHostV1 for RegistryBoundComputerUseMcpHostV1 {
    fn handle(
        &self,
        caller: &ComputerUseCallerV1,
        request: Request,
        cancelled: &AtomicBool,
    ) -> Response {
        self.handle_inner(caller, request, cancelled)
    }
}

#[cfg(test)]
#[path = "authorized_host/tests.rs"]
mod tests;
