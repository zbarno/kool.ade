mod core;
mod provider_configs;

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
        _resource_bridge: None,
        _sandbox: None,
        _planning_sandbox: None,
    }
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
