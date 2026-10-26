use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    thread,
    time::{Duration, Instant},
};

const MODEL_ID: &str = "ollama/llama3.2";

fn install_fake_opencode(root: &std::path::Path) -> std::path::PathBuf {
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then printf '%s\\n' 'ollama/llama3.2' 'ollama/missing-model' 'openai/gpt token=synthetic-secret'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"auth\" ] && [ \"$3\" = \"list\" ]; then printf '%s\\n' '┌ Credentials /synthetic/auth.json' '└ 0 credentials' '┌ Environment' '└ 0 environment variables'; exit 0; fi\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

fn write_config(root: &std::path::Path, base_url: &str) {
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        format!(r#"{{"provider":{{"ollama":{{"options":{{"baseURL":"{base_url}/v1"}}}}}}}}"#),
    )
    .unwrap();
}

fn configure_environment(
    root: &std::path::Path,
    binary: &std::path::Path,
) -> crate::test_support::ProcessEnvironmentGuard {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");
    environment
}

fn serve_catalog(body: &'static str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "local catalog request did not arrive"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("local catalog accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0_u8; 2048];
        let read = stream.read(&mut request).unwrap();
        let request = std::str::from_utf8(&request[..read]).unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.0\r\n"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    (format!("http://{address}"), server)
}

#[test]
fn local_model_routes_only_after_non_generative_loopback_catalog_check() {
    // Production boundary: full CLI adapter, isolated loopback model-list request, settings
    // persistence, and task-route validation. The synthetic HTTP server proves the endpoint check
    // actually runs; no external service or generated model request is used. Setup cost is one
    // short fake CLI and one local HTTP exchange.
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-local-model-live-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = install_fake_opencode(&root);
    let (base_url, server) = serve_catalog(r#"{"data":[{"id":"llama3.2"}]}"#);
    write_config(&root, &base_url.replace("127.0.0.1", "localhost"));
    let _environment = configure_environment(&root, &binary);

    let report = discover_provider("opencode", true);
    server.join().unwrap();
    assert_eq!(report.models, [MODEL_ID]);
    assert!(report.model_catalog_fresh);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    assert!(report.implementation_available);
    assert!(
        !report
            .models
            .iter()
            .any(|model| model.contains("synthetic-secret"))
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    assert_eq!(settings.default_harness, None);
    let detected = &settings.discovered["opencode"];
    assert!(
        super::super::super::super::super::has_current_provider_capabilities(
            "opencode",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    let route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some(MODEL_ID.into()),
        },
    )]);
    crate::app::root::harness_selection::validate_task_routes(&settings, &route).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_local_catalog_without_configured_model_creates_fresh_empty_evidence() {
    // Production boundary: a live local catalog that omits the configured ID must become a
    // model gap after adapter conversion, not authentication success or a routable capability.
    // The fake CLI and loopback server are the only dependencies; setup is one short exchange.
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-local-model-mismatch-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = install_fake_opencode(&root);
    let (base_url, server) = serve_catalog(r#"{"data":[{"id":"other-model"}]}"#);
    write_config(&root, &base_url);
    let _environment = configure_environment(&root, &binary);

    let report = discover_provider("opencode", true);
    server.join().unwrap();
    assert!(report.models.is_empty());
    assert!(report.model_catalog_fresh);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    assert!(
        report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("does not list the configured model")
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    let detected = &settings.discovered["opencode"];
    assert!(detected.model_catalog_fresh);
    assert!(detected.models.is_empty());
    assert!(
        super::super::super::super::super::has_current_provider_capabilities(
            "opencode",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    let missing_configured_model = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some(MODEL_ID.into()),
        },
    )]);
    assert!(
        crate::app::root::harness_selection::validate_task_routes(
            &settings,
            &missing_configured_model
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_model_remains_non_routable_when_loopback_endpoint_is_absent() {
    // Production boundary: adapter conversion and route validation with a configured but
    // unreachable local endpoint. A fake CLI and closed loopback port are the only dependencies;
    // setup is one subprocess and one bounded connection attempt.
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-local-model-absent-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = install_fake_opencode(&root);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    write_config(&root, &format!("http://{address}"));
    let _environment = configure_environment(&root, &binary);

    let report = discover_provider("opencode", true);
    assert!(report.models.contains(&MODEL_ID.to_owned()));
    assert!(!report.model_catalog_fresh);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    assert!(
        report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("did not return a usable model catalog")
    );
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    let detected = &settings.discovered["opencode"];
    assert!(detected.ready);
    assert!(!detected.model_catalog_fresh);
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    assert!(
        super::super::super::super::super::has_current_provider_capabilities(
            "opencode",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    let route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some(MODEL_ID.into()),
        },
    )]);
    assert!(crate::app::root::harness_selection::validate_task_routes(&settings, &route).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
