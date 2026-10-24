#[cfg(unix)]
mod cancellation;
mod core;
mod invocation_matrix;
mod launch_profile;
mod pi;
mod provider_configs;
#[cfg(unix)]
mod provider_mcp_e2e;
mod role_authority;

use super::super::{ApplicationBoundary, PrivateDirectory};
use crate::harness::ToolAccess;
use serde_json::Value;
use std::{fs, path::PathBuf};

fn test_boundary(access: ToolAccess) -> ApplicationBoundary {
    let scratch = PrivateDirectory::create("koolade-provider-test").unwrap();
    ApplicationBoundary {
        scratch,
        server_name: "koolade_test_0123456789abcdef".into(),
        app_binary: Some(std::env::current_exe().unwrap()),
        server_config: (access != ToolAccess::None).then(|| PathBuf::from("/tmp/server.json")),
        access,
        _session_bridge: None,
        _resource_bridge: None,
        execution_session: None,
        screenshot_review_audit: None,
    }
}

#[test]
fn successful_provider_configuration_records_local_telemetry_attribution() {
    let session = std::sync::Arc::new(crate::execution::session::telemetry_test_session());
    let mut boundary = test_boundary(ToolAccess::None);
    boundary.execution_session = Some(session.clone());
    let mut argv = Vec::new();
    let mut env = Vec::new();

    boundary
        .configure(super::CliProvider::Codex, &mut argv, &mut env)
        .unwrap();
    boundary.set_telemetry_model(Some("gpt-5-codex"));

    assert_eq!(
        session.telemetry_attribution_for_test(),
        Some(("codex".into(), Some("gpt-5-codex".into())))
    );
}

#[test]
fn every_supported_provider_has_a_stable_telemetry_id() {
    for (provider, expected) in [
        (super::CliProvider::Codex, "codex"),
        (super::CliProvider::Claude, "claude"),
        (super::CliProvider::Antigravity, "antigravity"),
        (super::CliProvider::OpenCode, "opencode"),
        (super::CliProvider::Copilot, "copilot"),
        (super::CliProvider::Pi, "pi"),
    ] {
        assert_eq!(provider.telemetry_id(), expected);
    }
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
