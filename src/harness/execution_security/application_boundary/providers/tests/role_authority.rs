use super::super::{CliProvider, configure};
use super::test_boundary;
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
        let expected_tools = tool_names(expected_access);
        for provider in PROVIDERS {
            let boundary = test_boundary(expected_access);
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
            ExecutionMode::Planning,
            ToolAccess::ReadOnly,
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
            ExecutionMode::Implementation,
            ToolAccess::BoundedImplementation,
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
        let tools = tool_names(access);
        for provider in PROVIDERS {
            let boundary = test_boundary(role.harness_mode().tool_access());
            let mut args = Vec::new();
            let mut env = Vec::new();
            configure(&boundary, provider, &mut args, &mut env).unwrap();
            assert_eq!(boundary.mcp_tool_names(), tools, "{provider:?} {role:?}");
            assert_provider_tool_authority(provider, &boundary, &args, &tools);
        }
    }
}

fn expected_execution_mode(role: HarnessRole) -> crate::execution::contract::ExecutionModeV1 {
    use crate::execution::contract::ExecutionModeV1;
    match role {
        HarnessRole::Implementer | HarnessRole::RepairWorker | HarnessRole::PRPreparer => {
            ExecutionModeV1::Implementation
        }
        HarnessRole::RuntimeQaReviewer => ExecutionModeV1::RuntimeQa,
        HarnessRole::Manager
        | HarnessRole::PlanningAnalyst
        | HarnessRole::PlanReviewer
        | HarnessRole::TaskDecomposer
        | HarnessRole::CodeReviewer
        | HarnessRole::SpecReconciler => ExecutionModeV1::PlanningReadOnly,
    }
}

fn tool_names(access: ToolAccess) -> Vec<String> {
    match access {
        ToolAccess::None => Vec::new(),
        ToolAccess::ReadOnly => vec!["koolade_bash".into()],
        ToolAccess::BoundedImplementation => vec![
            "koolade_bash".into(),
            "koolade_resource".into(),
            "koolade_dependency".into(),
        ],
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
