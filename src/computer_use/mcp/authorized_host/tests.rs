use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Operations(AtomicUsize);

impl ComputerUseMcpOperationsV1 for Operations {
    fn capabilities(&self, _caller: &ComputerUseCallerV1) -> Response {
        response(Status::Ready)
    }

    fn consent_scope(
        &self,
        _caller: &ComputerUseCallerV1,
    ) -> Result<super::super::ComputerUseConsentScopeV1, Box<Response>> {
        Ok(super::super::ComputerUseConsentScopeV1 {
            machine_id: "00000000-0000-4000-8000-000000000011".into(),
            approved_destination: None,
            allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
            allowed_apps: vec!["chromium".into(), "test-gui-fixture".into()],
            network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        })
    }

    fn request_session(
        &self,
        _caller: &ComputerUseCallerV1,
        _grant: &super::super::ComputerUseConsentGrantV1,
        _cancelled: &AtomicBool,
    ) -> Response {
        self.0.fetch_add(1, Ordering::Relaxed);
        response(Status::Blocked)
    }

    fn dispatch_authorized(
        &self,
        _caller: &ComputerUseCallerV1,
        _authority: Arc<AuthorizedComputerUseSessionV1>,
        _request: Request,
        _cancelled: &AtomicBool,
    ) -> Response {
        self.0.fetch_add(1, Ordering::Relaxed);
        response(Status::Completed)
    }
}

struct RegisterThenFinish {
    registry: Arc<ComputerUseAuthorityRegistryV1>,
    status: Status,
    cancel_during_startup: bool,
}

impl ComputerUseMcpOperationsV1 for RegisterThenFinish {
    fn capabilities(&self, _caller: &ComputerUseCallerV1) -> Response {
        response(Status::Ready)
    }

    fn consent_scope(
        &self,
        _caller: &ComputerUseCallerV1,
    ) -> Result<super::super::ComputerUseConsentScopeV1, Box<Response>> {
        Ok(super::super::ComputerUseConsentScopeV1 {
            machine_id: "00000000-0000-4000-8000-000000000011".into(),
            approved_destination: None,
            allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
            allowed_apps: vec!["chromium".into()],
            network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        })
    }

    fn request_session(
        &self,
        caller: &ComputerUseCallerV1,
        grant: &super::super::ComputerUseConsentGrantV1,
        cancelled: &AtomicBool,
    ) -> Response {
        register_session(&self.registry, caller, grant, false);
        if self.cancel_during_startup {
            cancelled.store(true, Ordering::Release);
        }
        Response {
            session_id: (self.status == Status::Ready).then(|| caller.identity.session_id.clone()),
            ..response(self.status)
        }
    }

    fn dispatch_authorized(
        &self,
        _caller: &ComputerUseCallerV1,
        _authority: Arc<AuthorizedComputerUseSessionV1>,
        _request: Request,
        _cancelled: &AtomicBool,
    ) -> Response {
        response(Status::Blocked)
    }
}

fn response(status: Status) -> Response {
    Response {
        schema_version: super::super::COMPUTER_USE_MCP_VERSION_V1,
        status,
        summary: "synthetic test result".into(),
        reason_code: None,
        session_id: None,
        capabilities: vec![],
        observation: None,
        action_outcome: None,
    }
}

fn caller() -> ComputerUseCallerV1 {
    ComputerUseCallerV1 {
        identity: crate::computer_use::ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            user_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
        },
        actor: crate::computer_use::ComputerActionActorV1::Agent,
        role_id: "implementer/v1".into(),
    }
}

fn approved_grant(
    caller: &ComputerUseCallerV1,
    approved_at_unix_ms: u64,
) -> super::super::ComputerUseConsentGrantV1 {
    super::super::ComputerUseConsentGrantV1 {
        request: super::super::ComputerUseConsentRequestV1 {
            request_id: "00000000-0000-4000-8000-000000000009".into(),
            identity: caller.identity.clone(),
            role_id: caller.role_id.clone(),
            purpose: "inspect a synthetic application".into(),
            machine_id: "00000000-0000-4000-8000-000000000011".into(),
            approved_destination: None,
            allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
            allowed_apps: vec!["chromium".into()],
            network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        },
        approved_at_unix_ms,
        expires_at_unix_ms: approved_at_unix_ms + 60_000,
    }
}

fn register_session(
    registry: &ComputerUseAuthorityRegistryV1,
    caller: &ComputerUseCallerV1,
    grant: &super::super::ComputerUseConsentGrantV1,
    add_unapproved_mouse_input: bool,
) {
    use crate::computer_use::{
        ComputerCapabilityV1, ComputerControllerProofV1, ComputerLeaseV1, ComputerLifecycleV1,
        ComputerNetworkProfileV1, ComputerOperationV1, ComputerPolicyV1, ComputerSessionClassV1,
        ComputerSessionV1, ComputerTargetV1,
    };

    let signer = ed25519_dalek::SigningKey::from_bytes(&[19; 32]);
    let key_id = "00000000-0000-4000-8000-000000000010".to_owned();
    let identity = caller.identity.clone();
    let mut allowed_operations = grant
        .request
        .allowed_operations
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if add_unapproved_mouse_input {
        allowed_operations.insert(ComputerOperationV1::MouseInput);
    }
    let allowed_apps = std::collections::BTreeSet::from(["chromium".to_owned()]);
    let budget = grant.request.resource_budget.clone();
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations: allowed_operations,
        approval_generation: 1,
        resource_budget: budget,
    };
    let mut session = ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: grant.approved_at_unix_ms,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalKvm {
            machine_id: "00000000-0000-4000-8000-000000000011".into(),
        },
        image_digest: format!("sha256:{}", "a".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "b".repeat(64)),
        image_signer_key_id: "00000000-0000-4000-8000-000000000012".into(),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps,
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: identity.user_id.clone(),
            lease_id: "00000000-0000-4000-8000-000000000013".into(),
            generation: 1,
            issued_at_unix_ms: grant.approved_at_unix_ms,
            expires_at_unix_ms: grant.expires_at_unix_ms,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: vec![],
        controller_proof: ComputerControllerProofV1 {
            signer_key_id: key_id.clone(),
            signature_hex: "0".repeat(128),
        },
    };
    session.attest(key_id.clone(), &signer).unwrap();
    registry
        .register(
            session,
            key_id,
            signer.verifying_key(),
            caller.actor,
            caller.role_id.clone(),
            unix_time_ms(),
        )
        .unwrap();
}

#[path = "tests/consent_flow.rs"]
mod consent_flow;
#[path = "tests/grant_scope.rs"]
mod grant_scope;
