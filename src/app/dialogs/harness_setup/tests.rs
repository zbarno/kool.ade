use super::*;
use crate::persistence::harness_settings::HarnessSettings;

#[cfg(unix)]
#[test]
fn manual_executable_path_requires_execute_permission_and_canonicalizes() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("harness-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("pi");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!is_executable_file(&binary));
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(is_executable_file(&binary.canonicalize().unwrap()));
    assert!(!is_executable_file(&root));
    let _ = std::fs::remove_dir_all(root);
}

fn report(ok: bool) -> HarnessProbe {
    HarnessProbe {
        id: "pi".into(),
        status: if ok { "pi 0.30.1" } else { "pi (unavailable)" }.into(),
        diagnostic: if ok {
            None
        } else {
            Some("login required".into())
        },
        executable: Some("/example/bin/pi".into()),
        version: ok.then(|| "0.30.1".into()),
        ready: ok,
        models: vec!["configured-model".into()],
        default_model: ok.then(|| "configured-model".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        authentication: if ok {
            crate::execution::contract::ReadinessState::Ready
        } else {
            crate::execution::contract::ReadinessState::Unknown
        },
        implementation_available: true,
    }
}

mod discovery;
mod persistence;
mod ui;
