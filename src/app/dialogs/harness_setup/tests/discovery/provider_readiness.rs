use super::super::{HarnessSettings, invalidate_discovery, report};

#[cfg(unix)]
#[test]
fn verified_profiles_enable_provider_roles_without_a_docker_target() {
    use crate::execution::contract::{HarnessRole, ReadinessState};
    use std::fs;

    let root =
        std::env::temp_dir().join(format!("harness-profile-matrix-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    for (id, version) in [
        ("pi", "0.30.1"),
        ("opencode", "1.0.0"),
        ("copilot", "1.0.0"),
        ("antigravity", "1.0.0"),
    ] {
        let binary = root.join(id);
        fs::write(&binary, "synthetic provider executable").unwrap();
        let mut provider = report(true);
        provider.id = id.into();
        provider.executable = Some(binary.to_string_lossy().into_owned());
        provider.version = Some(version.into());
        provider.implementation_available = false;
        let mut settings = HarnessSettings::default();
        super::super::super::discovery::apply_probe_results(
            &mut settings,
            std::slice::from_ref(&provider),
            None,
            1,
        );
        let snapshot = settings.discovered[id]
            .capability_snapshot
            .as_ref()
            .unwrap();

        assert_eq!(
            snapshot.scriptable_invocation,
            ReadinessState::Ready,
            "{id}"
        );
        assert_eq!(
            snapshot.docker_mcp_bridge,
            ReadinessState::Unavailable,
            "{id}"
        );
        assert_eq!(
            snapshot.execution_target,
            ReadinessState::Unavailable,
            "{id}"
        );
        assert!(
            snapshot
                .modes
                .iter()
                .all(|mode| mode.available == ReadinessState::Ready),
            "{id}"
        );
        assert!(
            snapshot
                .roles
                .iter()
                .filter(|role| role.role.requires_execution_target())
                .all(|role| role.available == ReadinessState::Unavailable),
            "{id}"
        );
        assert!(
            snapshot
                .roles
                .iter()
                .filter(|role| !role.role.requires_execution_target())
                .all(|role| role.available == ReadinessState::Ready),
            "{id}"
        );
        assert!(snapshot.supports_role(HarnessRole::SpecReconciler), "{id}");
        assert!(!snapshot.supports_role(HarnessRole::Implementer), "{id}");

        let mut refreshed_report = provider.clone();
        super::super::super::discovery::update_runtime_readiness(
            &mut settings,
            std::slice::from_mut(&mut refreshed_report),
            true,
        );
        let refreshed = settings.discovered[id]
            .capability_snapshot
            .as_ref()
            .unwrap();
        assert_eq!(refreshed.docker_mcp_bridge, ReadinessState::Ready, "{id}");
        assert!(refreshed.supports_role(HarnessRole::Implementer), "{id}");
    }
    let mut codex = report(true);
    codex.id = "codex".into();
    codex.executable = Some(root.join("codex").to_string_lossy().into_owned());
    fs::write(root.join("codex"), "synthetic provider executable").unwrap();
    let mut codex_settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(
        &mut codex_settings,
        &[codex.clone()],
        None,
        1,
    );
    let codex_snapshot = codex_settings.discovered["codex"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert!(
        codex_snapshot
            .modes
            .iter()
            .all(|mode| mode.available == ReadinessState::Unavailable)
    );
    assert!(
        codex_snapshot
            .roles
            .iter()
            .all(|role| role.available == ReadinessState::Unavailable)
    );
    let mut codex_report = codex;
    super::super::super::discovery::update_runtime_readiness(
        &mut codex_settings,
        std::slice::from_mut(&mut codex_report),
        true,
    );
    let refreshed_codex = codex_settings.discovered["codex"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert!(
        refreshed_codex
            .modes
            .iter()
            .all(|mode| mode.available == ReadinessState::Unavailable)
    );
    assert!(
        refreshed_codex
            .roles
            .iter()
            .all(|role| role.available == ReadinessState::Unavailable)
    );

    let mut claude = report(true);
    claude.id = "claude".into();
    claude.executable = Some("/synthetic/bin/claude".into());
    let mut claude_settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(&mut claude_settings, &[claude], None, 1);
    let claude_snapshot = claude_settings.discovered["claude"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert!(
        claude_snapshot
            .modes
            .iter()
            .all(|mode| mode.available == ReadinessState::Unavailable)
    );
    assert_eq!(claude_snapshot.cli_binary, ReadinessState::Ready);
    assert_eq!(claude_snapshot.authentication, ReadinessState::Ready);
    assert_eq!(claude_snapshot.model_catalog, ReadinessState::Ready);
    assert!(
        claude_snapshot
            .roles
            .iter()
            .all(|role| role.available == ReadinessState::Unavailable)
    );
    let mut claude_report = report(true);
    claude_report.id = "claude".into();
    super::super::super::discovery::update_runtime_readiness(
        &mut claude_settings,
        std::slice::from_mut(&mut claude_report),
        true,
    );
    let refreshed_claude = claude_settings.discovered["claude"]
        .capability_snapshot
        .as_ref()
        .unwrap();
    assert!(
        refreshed_claude
            .roles
            .iter()
            .all(|role| role.available == ReadinessState::Unavailable)
    );
    assert_eq!(
        refreshed_claude.docker_mcp_bridge,
        ReadinessState::Unavailable
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn full_refresh_keeps_cached_provider_results_until_each_result_arrives() {
    let pi = report(true);
    let mut claude = report(true);
    claude.id = "claude".into();
    claude.executable = Some("/example/bin/claude".into());
    let mut settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[pi.clone(), claude],
        None,
        1,
    );
    let before = settings.discovered.clone();

    invalidate_discovery(&mut settings, None);

    assert_eq!(settings.discovered, before);
    assert!(settings.discovered["pi"].ready);
    assert!(settings.discovered["pi"].capability_snapshot.is_some());
    assert!(settings.discovered["claude"].ready);
    assert!(settings.discovered["claude"].capability_snapshot.is_some());
}

#[test]
fn successful_discovery_persists_version_without_implicitly_selecting_a_default() {
    let mut settings = HarnessSettings::default();
    super::super::super::discovery::apply_probe_results(&mut settings, &[report(true)], None, 1);
    assert_eq!(settings.default_harness, None);
    assert_eq!(settings.discovered["pi"].version.as_deref(), Some("0.30.1"));
    assert!(settings.discovered["pi"].ready);
    assert_eq!(
        settings.discovered["pi"].default_model.as_deref(),
        Some("configured-model")
    );
}
