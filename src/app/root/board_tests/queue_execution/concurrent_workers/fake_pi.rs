use std::path::{Path, PathBuf};

fn authenticated_capability_snapshot() -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };

    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: "pi".into(),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: crate::execution::contract::HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    }
}

pub(super) fn sandbox_available() -> bool {
    crate::harness::runtime_capabilities::RuntimeCapabilities::detect().implementation
}

pub(super) struct RestoreEnvironmentVariable {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl Drop for RestoreEnvironmentVariable {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

pub(super) fn restore_environment_variable(name: &'static str) -> RestoreEnvironmentVariable {
    RestoreEnvironmentVariable {
        name,
        previous: std::env::var_os(name),
    }
}

pub(super) struct HarnessEnvironment {
    _home: RestoreEnvironmentVariable,
    _pi_config: RestoreEnvironmentVariable,
}

pub(super) fn configure_harness_settings(root: &Path) -> HarnessEnvironment {
    let restore = restore_environment_variable("KOOLADE_HOME");
    let restore_pi_config = restore_environment_variable("PI_CODING_AGENT_DIR");
    let home = root.join("app-home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("KOOLADE_HOME", &home) };
    let pi_config = root.join("pi-agent-config");
    std::fs::create_dir_all(&pi_config).unwrap();
    std::fs::write(
        pi_config.join("settings.json"),
        r#"{"defaultProvider":"fixture","defaultModel":"fixture-model"}"#,
    )
    .unwrap();
    std::fs::write(
        pi_config.join("models.json"),
        r#"{"providers":{"fixture":{"models":[{"id":"fixture-model","name":"Fixture Model"}]}}}"#,
    )
    .unwrap();
    unsafe { std::env::set_var("PI_CODING_AGENT_DIR", &pi_config) };
    crate::persistence::harness_settings::save(
        &crate::persistence::harness_settings::HarnessSettings {
            discovered: std::collections::BTreeMap::from([(
                "pi".into(),
                crate::persistence::harness_settings::DetectedHarness {
                    status: "pi fixture 1.0".into(),
                    version: Some("1.0".into()),
                    executable: None,
                    diagnostic: None,
                    ready: true,
                    models: Vec::new(),
                    default_model: None,
                    model_catalog_fresh: false,
                    configuration_required: false,
                    implementation_available: true,
                    checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                    capability_snapshot: Some(authenticated_capability_snapshot()),
                },
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    HarnessEnvironment {
        _home: restore,
        _pi_config: restore_pi_config,
    }
}

pub(super) fn create(root: &Path) -> PathBuf {
    let pi = root.join("pi-fixture");
    std::fs::write(&pi, include_str!("fake_pi/provider.py")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    pi
}

#[cfg(test)]
#[path = "fake_pi/tests.rs"]
mod tests;
