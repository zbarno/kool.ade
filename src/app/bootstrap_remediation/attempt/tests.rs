use super::*;
use crate::app::bootstrap_remediation::{
    identity::HostIdentity, plan::RemediationPlan, request::RemediationRequest,
};
use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};

#[test]
fn windows_wsl_authorization_scope_uses_fixed_operation_code() {
    let identity =
        HostIdentity::from_facts(Host::Windows, "23H2", "synthetic-machine", "synthetic-user")
            .unwrap();
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::EnableWindowsWslFeatures,
        host: Host::Windows,
        prerequisite_id: "setup:windows:wsl-features".into(),
        image_profile: None,
    };
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    let plan = RemediationPlan::build_for_test(&request, &identity).unwrap();
    let scopes = bound_scopes(&plan, &identity).unwrap();
    let operation_scope = "setup:remediation:operation:windows_enable_wsl_features";
    assert!(scopes.iter().any(|scope| scope == operation_scope));

    let with_operation = crate::persistence::setup_tasks::authorization_scope_digest(
        &plan.prerequisite_id,
        "synthetic-attempt",
        plan.operation_code_version,
        &scopes,
    );
    let without_operation = scopes
        .iter()
        .filter(|scope| scope.as_str() != operation_scope)
        .cloned()
        .collect::<Vec<_>>();
    let without_operation = crate::persistence::setup_tasks::authorization_scope_digest(
        &plan.prerequisite_id,
        "synthetic-attempt",
        plan.operation_code_version,
        &without_operation,
    );
    assert_ne!(with_operation, without_operation);
}
