use super::super::{CliProvider, configure};
use super::test_boundary;
use crate::harness::ToolAccess;

#[test]
fn pi_only_registers_read_only_application_mcp_for_planning() {
    let boundary = test_boundary(ToolAccess::ReadOnly);
    let mut argv = Vec::new();
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Pi, &mut argv, &mut env).unwrap();
    assert!(argv.contains(&"--no-builtin-tools".into()));
    assert!(
        argv.windows(2)
            .any(|args| args == ["--tools", "koolade_bash"])
    );
    assert!(!argv.iter().any(|arg| arg == "read,grep,find,ls"));
    assert!(
        !argv
            .iter()
            .any(|arg| arg == "koolade_resource,koolade_dependency")
    );
    assert!(argv.iter().any(|arg| arg == "--extension"));
    assert!(env.iter().any(|(key, _)| key == "KOOLADE_MCP_COMMAND"));
    let extension =
        std::fs::read_to_string(boundary.working_directory().join("pi-koolade-mcp.ts")).unwrap();
    assert!(extension.contains("tools/call"));
    assert!(extension.contains("KOOLADE_MCP_TOOLS"));
    assert!(!extension.contains("node:fs"));
    assert!(!extension.contains("node:child_process.*exec"));
}

#[test]
fn pi_implementation_registers_only_application_owned_brokered_tools() {
    let boundary = test_boundary(ToolAccess::BoundedImplementation);
    let mut argv = Vec::new();
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Pi, &mut argv, &mut env).unwrap();
    assert!(argv.contains(&"--no-builtin-tools".into()));
    assert!(argv.windows(2).any(|args| {
        args == [
            "--tools",
            "koolade_bash,koolade_resource,koolade_dependency",
        ]
    }));
    for native_tool in ["read", "write", "edit", "bash", "grep", "find", "ls"] {
        assert!(!argv.iter().any(|arg| arg == native_tool));
    }
    let tools = env
        .iter()
        .find(|(key, _)| key == "KOOLADE_MCP_TOOLS")
        .map(|(_, value)| serde_json::from_str::<Vec<String>>(value).unwrap())
        .unwrap();
    assert_eq!(
        tools,
        ["koolade_bash", "koolade_resource", "koolade_dependency"]
    );
    assert!(!env.iter().any(|(key, _)| {
        key.contains("API_KEY") || key.contains("TOKEN") || key.contains("SECRET")
    }));
}

#[test]
fn pi_runtime_qa_reviewer_registers_the_screenshot_inspection_tool() {
    let boundary = test_boundary(ToolAccess::RuntimeQaScreenshots);
    let mut argv = Vec::new();
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Pi, &mut argv, &mut env).unwrap();

    assert!(argv.contains(&"--no-builtin-tools".into()));
    assert!(
        argv.windows(2)
            .any(|args| { args == ["--tools", "koolade_runtime_qa_screenshot"] })
    );
    let tools = env
        .iter()
        .find(|(key, _)| key == "KOOLADE_MCP_TOOLS")
        .map(|(_, value)| serde_json::from_str::<Vec<String>>(value).unwrap())
        .unwrap();
    assert_eq!(tools, ["koolade_runtime_qa_screenshot"]);

    let extension =
        std::fs::read_to_string(boundary.working_directory().join("pi-koolade-mcp.ts")).unwrap();
    assert!(extension.contains("allowedTools.has(\"koolade_runtime_qa_screenshot\")"));
    assert!(extension.contains("screenshot_id: Type.String"));
    assert!(
        extension
            .lines()
            .all(|line| !line.trim_start().starts_with("register(\"koolade_bash\""))
    );
}

#[test]
fn pi_without_an_application_session_has_no_tools_or_extension() {
    let boundary = test_boundary(ToolAccess::None);
    let mut argv = Vec::new();
    let mut env = Vec::new();
    configure(&boundary, CliProvider::Pi, &mut argv, &mut env).unwrap();
    assert_eq!(argv, ["--no-tools"]);
    assert!(env.is_empty());
}
