/// Production boundary: stale saved-route selection requests the app-owned
/// discovery scheduler, whose worker refreshes persisted proof without a
/// Coding Tools dialog. A parser test cannot prove route-triggered admission
/// or worker-result persistence. Uses one synthetic local CLI, temporary HOME
/// and settings, and bounded subprocesses; no provider account, Docker, or
/// network dependency.
#[test]
fn stale_manager_route_requests_and_persists_automatic_provider_refresh() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, MANAGER, WorkRoute,
    };
    use std::{os::unix::fs::PermissionsExt, time::Instant};

    let root = std::env::temp_dir().join(format!(
        "koolade-auto-capability-refresh-{}",
        uuid::Uuid::new_v4()
    ));
    let home = root.join("home");
    let agent = home.join(".pi/agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"model-a"}"#,
    )
    .unwrap();
    std::fs::write(
        agent.join("models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"model-a"}]}}}"#,
    )
    .unwrap();
    let pi = root.join("pi");
    std::fs::write(
        &pi,
        "#!/bin/sh\ncase \"$1\" in\n--version) echo 'pi 1.2.3'; exit 0;;\n--help) printf '%s\\n' --print --mode --no-session --no-approve --append-system-prompt --thinking --provider --model --no-extensions --no-skills --no-prompt-templates --no-themes --no-context-files --no-tools --no-builtin-tools --tools --extension json xhigh; exit 0;;\nauth) echo '{\"status\":\"ready\",\"provider\":\"synthetic\",\"authType\":\"api_key\"}'; exit 0;;\nesac\nexit 3\n",
    )
    .unwrap();
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();

    let settings_path = root.join("harness-settings.json");
    let settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([(
            MANAGER.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("model-a".into()),
            },
        )]),
        discovered: std::collections::BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi ready".into(),
                version: Some("1.2.2".into()),
                executable: Some(pi.display().to_string()),
                diagnostic: None,
                ready: true,
                models: vec!["model-a".into()],
                default_model: Some("model-a".into()),
                model_catalog_fresh: false,
                configuration_required: false,
                implementation_available: false,
                capability_snapshot: None,
                checked_at_unix_ms: 1,
                verified_at_unix_ms: None,
            },
        )]),
        ..HarnessSettings::default()
    };

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", &home);
    environment.set("PI_CODING_AGENT_DIR", &agent);
    environment.set(crate::harness::pi_harness::PI_BINARY_ENV, &pi);
    environment.set("PATH", "/usr/bin:/bin");

    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let route = crate::app::root::harness_selection::configured_harness_for_project(
            &mut None,
            Some(MANAGER),
            &std::collections::BTreeMap::new(),
        );
        assert!(route.route_blocker().is_some());
        let binding = route
            .capability_refresh_binding()
            .expect("stale saved route retains its exact refresh binding");
        assert_eq!(binding.provider_id, "pi");
        assert_eq!(binding.selected_model, "model-a");
        assert_eq!(binding.observed_at_unix_ms, 1);

        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        loop {
            crate::app::dialogs::with_cli_background_sweeps_permitted_for_test(|| {
                crate::app::dialogs::poll_cli_auto_refresh_for_test();
            });
            let persisted = crate::persistence::harness_settings::load().0;
            if persisted.discovered.get("pi").is_some_and(|detected| {
                detected.has_current_authenticated_capability(
                    "pi",
                    crate::persistence::harness_settings::current_unix_ms(),
                ) && detected.version.as_deref() == Some("1.2.3")
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "provider refresh was not durably committed within 30s"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let refreshed = crate::persistence::harness_settings::load().0;
        let detected = &refreshed.discovered["pi"];
        let now = crate::persistence::harness_settings::current_unix_ms();
        assert!(detected.has_current_authenticated_capability("pi", now));
        assert!(detected.has_verified_model_at("pi", "model-a", now));
        assert_eq!(detected.version.as_deref(), Some("1.2.3"));
        assert_eq!(
            crate::app::root::harness_selection::capability_refresh_route_state(
                &binding,
                &std::collections::BTreeMap::new(),
                &std::collections::BTreeMap::new(),
            ),
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Ready,
            "automatic proof refresh keeps the original provider, role, and model route"
        );
    });

    let _ = std::fs::remove_dir_all(root);
}
