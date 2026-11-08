use super::*;
use crate::{
    computer_use::{
        ComputerActionActorV1, ComputerIdentityV1,
        mcp::{
            COMPUTER_USE_MCP_VERSION_V1, ComputerUseCallerV1, ComputerUseMcpHostV1,
            ComputerUseMcpRequestV1, ComputerUseMcpResponseV1, ComputerUseMcpStatusV1,
        },
    },
    harness::{ComputerUseMcpAccessV1, ExecutionMode},
};
use std::sync::atomic::AtomicBool;

struct FixtureHost;

impl ComputerUseMcpHostV1 for FixtureHost {
    fn handle(
        &self,
        _: &ComputerUseCallerV1,
        _: ComputerUseMcpRequestV1,
        _: &AtomicBool,
    ) -> ComputerUseMcpResponseV1 {
        ComputerUseMcpResponseV1 {
            schema_version: COMPUTER_USE_MCP_VERSION_V1,
            status: ComputerUseMcpStatusV1::Denied,
            summary: "Denied".into(),
            reason_code: Some("denied".into()),
            session_id: None,
            capabilities: vec![],
            observation: None,
            action_outcome: None,
        }
    }
}

fn access(role_id: &str) -> anyhow::Result<ComputerUseMcpAccessV1> {
    ComputerUseMcpAccessV1::new(
        ComputerUseCallerV1 {
            identity: ComputerIdentityV1 {
                project_id: "00000000-0000-4000-8000-000000000001".into(),
                task_id: "00000000-0000-4000-8000-000000000002".into(),
                user_id: "00000000-0000-4000-8000-000000000003".into(),
                session_id: "00000000-0000-4000-8000-000000000004".into(),
            },
            actor: ComputerActionActorV1::Agent,
            role_id: role_id.into(),
        },
        std::sync::Arc::new(FixtureHost),
    )
}

#[test]
fn computer_use_grants_are_bound_to_an_eligible_mode_and_exact_role_manifest() {
    let implementer = access("implementer/v1").unwrap();
    assert!(
        validate_computer_use_invocation(
            &implementer,
            ExecutionMode::Planning,
            Some("00000000-0000-4000-8000-000000000002"),
            Some("implementer/v1"),
            None,
        )
        .is_err()
    );
    assert!(
        validate_computer_use_invocation(
            &implementer,
            ExecutionMode::Implementation,
            Some("00000000-0000-4000-8000-000000000002"),
            Some("repair/v1"),
            None,
        )
        .is_err()
    );
    assert!(access("manager/v9").is_err());
}
