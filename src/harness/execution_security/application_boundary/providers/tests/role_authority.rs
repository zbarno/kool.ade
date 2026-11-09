use super::super::{CliProvider, configure};
use crate::{
    execution::contract::HarnessRole,
    harness::{ExecutionMode, ToolAccess},
};
use serde_json::Value;

const PROVIDERS: [CliProvider; 6] = [
    CliProvider::Codex,
    CliProvider::Claude,
    CliProvider::Antigravity,
    CliProvider::OpenCode,
    CliProvider::Copilot,
    CliProvider::Pi,
];

#[test]
fn every_mode_has_equal_repository_tool_authority_across_all_providers() {
    for mode in ExecutionMode::ALL {
        let expected_access = mode.tool_access();
        let expected_tools = if expected_access == ToolAccess::InvestigationBroker {
            Vec::new()
        } else {
            tool_names(expected_access)
        };
        for provider in PROVIDERS {
            let boundary = super::test_boundary_for_profile(expected_access, mode.profile());
            let mut args = Vec::new();
            let mut env = Vec::new();
            configure(&boundary, provider, &mut args, &mut env).unwrap();

            assert_eq!(
                boundary.mcp_tool_names(),
                expected_tools,
                "{provider:?} {mode:?}"
            );
            assert_provider_tool_authority(provider, &boundary, &args, &expected_tools);
        }
    }
}

#[test]
fn every_user_role_keeps_its_intended_mode_and_provider_authority() {
    let expected = [
        (
            HarnessRole::Manager,
            ExecutionMode::Manager,
            ToolAccess::InvestigationBroker,
        ),
        (
            HarnessRole::PlanningAnalyst,
            ExecutionMode::Planning,
            ToolAccess::ReadOnly,
        ),
        (
            HarnessRole::PlanReviewer,
            ExecutionMode::Investigation,
            ToolAccess::ReadOnly,
        ),
        (
            HarnessRole::DecisionExplainer,
            ExecutionMode::DecisionExplanation,
            ToolAccess::None,
        ),
        (
            HarnessRole::TaskDecomposer,
            ExecutionMode::TaskGeneration,
            ToolAccess::ReadOnly,
        ),
        (
            HarnessRole::Implementer,
            ExecutionMode::Implementation,
            ToolAccess::BoundedImplementation,
        ),
        (
            HarnessRole::CodeReviewer,
            ExecutionMode::Investigation,
            ToolAccess::ReadOnly,
        ),
        (
            HarnessRole::RuntimeQaReviewer,
            ExecutionMode::ReadOnlyAnalysis,
            ToolAccess::None,
        ),
        (
            HarnessRole::SpecReconciler,
            ExecutionMode::Reconciliation,
            ToolAccess::None,
        ),
        (
            HarnessRole::RepairWorker,
            ExecutionMode::Implementation,
            ToolAccess::BoundedImplementation,
        ),
        (
            HarnessRole::PRPreparer,
            ExecutionMode::ReadOnlyAnalysis,
            ToolAccess::None,
        ),
    ];
    assert_eq!(expected.len(), HarnessRole::ALL.len());
    for role in HarnessRole::ALL {
        assert!(
            expected
                .iter()
                .any(|(expected_role, _, _)| *expected_role == role)
        );
    }

    for (role, mode, access) in expected {
        assert_eq!(role.harness_mode(), mode, "{role:?}");
        assert_eq!(mode.tool_access(), access, "{role:?}");
        assert_eq!(
            role.execution_mode(),
            expected_execution_mode(role),
            "{role:?}"
        );
        let tools = if access == ToolAccess::InvestigationBroker {
            Vec::new()
        } else {
            tool_names(access)
        };
        for provider in PROVIDERS {
            let boundary = super::test_boundary_for_profile(
                role.harness_mode().tool_access(),
                role.harness_mode().profile(),
            );
            let mut args = Vec::new();
            let mut env = Vec::new();
            configure(&boundary, provider, &mut args, &mut env).unwrap();
            assert_eq!(boundary.mcp_tool_names(), tools, "{provider:?} {role:?}");
            assert_provider_tool_authority(provider, &boundary, &args, &tools);
        }
    }
}

#[cfg(unix)]
#[test]
fn computer_use_tools_have_the_same_six_provider_surface() {
    // Production boundary: each provider's launch configuration exposes the
    // same app-owned tool names without adding caller authority arguments.
    // A schema unit test cannot prove provider CLI/env configuration parity.
    // Dependencies: Unix sockets and temporary files only; setup is local and fast.
    use crate::{
        computer_use::{
            ComputerActionActorV1, ComputerIdentityV1,
            mcp::{
                COMPUTER_USE_MCP_VERSION_V1, ComputerUseCallerV1, ComputerUseMcpHostV1,
                ComputerUseMcpRequestV1, ComputerUseMcpResponseV1, ComputerUseMcpStatusV1,
            },
        },
        harness::ComputerUseMcpAccessV1,
    };
    use std::sync::{Arc, atomic::AtomicBool, mpsc};

    struct DenyHost;
    impl ComputerUseMcpHostV1 for DenyHost {
        fn handle(
            &self,
            _: &ComputerUseCallerV1,
            _: ComputerUseMcpRequestV1,
            _: &AtomicBool,
        ) -> ComputerUseMcpResponseV1 {
            ComputerUseMcpResponseV1 {
                schema_version: COMPUTER_USE_MCP_VERSION_V1,
                status: ComputerUseMcpStatusV1::Denied,
                summary: "Denied by fixture.".into(),
                reason_code: Some("denied".into()),
                session_id: None,
                capabilities: vec![],
                observation: None,
                action_outcome: None,
            }
        }
    }

    // The application-state scratch path is deliberately nested, and CI's
    // temp root can be long enough to exceed AF_UNIX's socket path limit.
    // Keep this bridge fixture under a short OS temp directory like the
    // provider MCP end-to-end fixture does.
    let socket_root = tempfile::Builder::new()
        .prefix("k-")
        .tempdir_in("/tmp")
        .unwrap();
    let mut boundary = super::test_boundary(ToolAccess::None);
    let task_id = "00000000-0000-4000-8000-000000000002";
    let caller = ComputerUseCallerV1 {
        identity: ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: task_id.into(),
            user_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
        },
        actor: ComputerActionActorV1::Agent,
        role_id: "implementer/v1".into(),
    };
    let access = Arc::new(ComputerUseMcpAccessV1::new(caller, Arc::new(DenyHost)).unwrap());
    let (progress, _events) = mpsc::channel();
    let bridge = crate::harness::execution_security::computer_use_bridge::ComputerUseBridge::start(
        socket_root.path(),
        access,
        Arc::new(AtomicBool::new(false)),
        progress,
    )
    .unwrap();
    boundary.server_config = Some(boundary.scratch.path.join("server.json"));
    boundary._computer_use_bridge = Some(bridge);
    let expected = crate::harness::execution_security::computer_use_bridge::tool_names();
    for provider in PROVIDERS {
        let mut args = Vec::new();
        let mut env = Vec::new();
        configure(&boundary, provider, &mut args, &mut env).unwrap();
        assert_eq!(boundary.mcp_tool_names(), expected, "{provider:?}");
        assert_provider_tool_authority(provider, &boundary, &args, &expected);
    }
}

fn expected_execution_mode(role: HarnessRole) -> crate::execution::contract::ExecutionModeV1 {
    use crate::execution::contract::ExecutionModeV1;
    match role {
        HarnessRole::Implementer | HarnessRole::RepairWorker => ExecutionModeV1::Implementation,
        HarnessRole::PRPreparer => ExecutionModeV1::PlanningReadOnly,
        HarnessRole::RuntimeQaReviewer => ExecutionModeV1::RuntimeQa,
        HarnessRole::Manager
        | HarnessRole::PlanningAnalyst
        | HarnessRole::PlanReviewer
        | HarnessRole::TaskDecomposer
        | HarnessRole::CodeReviewer
        | HarnessRole::DecisionExplainer
        | HarnessRole::SpecReconciler => ExecutionModeV1::PlanningReadOnly,
    }
}

fn tool_names(access: ToolAccess) -> Vec<String> {
    match access {
        ToolAccess::None => Vec::new(),
        ToolAccess::ReadOnly => vec!["koolade_bash".into()],
        ToolAccess::InvestigationBroker => vec!["koolade_investigate".into()],
        ToolAccess::BoundedImplementation => vec![
            "koolade_bash".into(),
            "koolade_resource".into(),
            "koolade_dependency".into(),
        ],
        ToolAccess::RuntimeQaScreenshots => vec!["koolade_runtime_qa_screenshot".into()],
    }
}

fn assert_provider_tool_authority(
    provider: CliProvider,
    boundary: &super::super::ApplicationBoundary,
    args: &[String],
    expected: &[String],
) {
    let server = boundary.server_name.as_str();
    match provider {
        CliProvider::Claude => {
            let allowed = args.iter().position(|arg| arg == "--allowedTools");
            match (expected.is_empty(), allowed) {
                (true, _) => assert!(args.windows(2).any(|pair| pair == ["--tools", ""])),
                (false, Some(index)) => assert_eq!(
                    args[index + 1],
                    expected
                        .iter()
                        .map(|tool| format!("mcp__{server}__{tool}"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                (false, None) => panic!("Claude omitted its app-owned tools for {expected:?}"),
            }
        }
        CliProvider::Copilot => {
            let actual = args
                .iter()
                .find_map(|arg| arg.strip_prefix("--available-tools="))
                .or_else(|| {
                    args.windows(2)
                        .find(|pair| pair[0] == "--available-tools")
                        .map(|pair| pair[1].as_str())
                })
                .unwrap_or_default();
            let expected = expected
                .iter()
                .map(|tool| format!("{server}-{tool}"))
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(actual, expected);
        }
        CliProvider::Pi => {
            if expected.is_empty() {
                assert!(args.iter().any(|arg| arg == "--no-tools"));
            } else {
                assert!(
                    args.windows(2)
                        .any(|pair| { pair[0] == "--tools" && pair[1] == expected.join(",") })
                );
            }
        }
        CliProvider::Codex => {
            for feature in [
                "code_mode_host",
                "code_mode",
                "code_mode_only",
                "shell_tool",
            ] {
                assert!(args.windows(2).any(|pair| pair == ["--disable", feature]));
            }
            if expected.is_empty() {
                assert!(args.iter().any(|arg| arg == "mcp_servers={}"));
            } else {
                assert!(args.iter().any(|arg| arg.contains(server)));
            }
        }
        CliProvider::Antigravity => {
            let config: Value =
                super::read_json(&boundary.scratch.path.join(".agents/mcp_config.json"));
            let servers = config["mcpServers"].as_object().unwrap();
            assert_eq!(
                servers.keys().map(String::as_str).collect::<Vec<_>>(),
                if expected.is_empty() {
                    vec![]
                } else {
                    vec![server]
                }
            );
        }
        CliProvider::OpenCode => {
            let config: Value = super::read_json(&boundary.scratch.path.join("opencode.json"));
            let servers = config["mcp"].as_object().unwrap();
            assert_eq!(
                servers.keys().map(String::as_str).collect::<Vec<_>>(),
                if expected.is_empty() {
                    vec![]
                } else {
                    vec![server]
                }
            );
            assert_eq!(config["tools"]["*"], false);
        }
    }
}
