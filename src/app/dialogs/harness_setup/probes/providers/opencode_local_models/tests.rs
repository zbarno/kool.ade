use super::managed::has_managed_config;
use super::*;
use super::{
    catalog::explicit_loopback_catalog_url,
    config::{merge_endpoints, parse_jsonc},
};

#[test]
fn local_endpoint_is_direct_loopback_only() {
    for endpoint in [
        "http://127.0.0.1:1234/v1",
        "http://[::1]:8000/v1",
        "http://localhost:11434/v1",
    ] {
        assert!(
            explicit_loopback_catalog_url(endpoint).is_some(),
            "{endpoint}"
        );
    }
    assert_eq!(
        explicit_loopback_catalog_url("http://localhost:11434/v1")
            .unwrap()
            .host_str(),
        Some("127.0.0.1")
    );
    for endpoint in [
        "https://api.example.test/v1",
        "http://localhost.example.test/v1",
        "https://[::1]:8000/v1",
        "file:///tmp/model",
        "not a url",
    ] {
        assert!(
            explicit_loopback_catalog_url(endpoint).is_none(),
            "{endpoint}"
        );
    }
}

#[test]
fn managed_config_presence_is_detected_without_host_paths() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-managed-path-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let present = root.join("managed.json");
    std::fs::write(&present, "{}").unwrap();
    assert_eq!(has_managed_config(&[present]), Some(true));
    assert_eq!(
        has_managed_config(&[root.join("missing.json")]),
        Some(false)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn jsonc_config_identifies_configured_loopback_and_remote_overrides() {
    let local = parse_jsonc(
        r#"{
            // Synthetic local endpoint.
            "provider": { "custom": { "options": { "baseURL": "http://127.0.0.1:11434/v1" } } }
        }"#,
    )
    .unwrap();
    let remote = parse_jsonc(
        r#"{"provider":{"custom":{"options":{"baseURL":"https://api.example.test/v1"}}}}"#,
    )
    .unwrap();
    let mut endpoints = BTreeMap::new();
    merge_endpoints(&mut endpoints, &local).unwrap();
    assert!(endpoints.get("custom").unwrap().loopback_configured);
    assert!(endpoints.get("custom").unwrap().catalog_url.is_some());
    merge_endpoints(&mut endpoints, &remote).unwrap();
    assert!(!endpoints.get("custom").unwrap().loopback_configured);
    assert!(endpoints.get("custom").unwrap().catalog_url.is_none());

    let credentialed_local = parse_jsonc(
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1","headers":{"Authorization":"Bearer synthetic-secret"}}}}}"#,
    )
    .unwrap();
    merge_endpoints(&mut endpoints, &credentialed_local).unwrap();
    let local = endpoints.get("ollama").unwrap();
    assert!(local.loopback_configured);
    assert!(local.catalog_url.is_some());
    assert!(!local.no_auth_required);
}

#[test]
fn remote_provider_override_and_sanitized_custom_config_fail_closed() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-locality-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    let config_path = root.join(".config/opencode/opencode.json");
    std::fs::write(
        &config_path,
        r#"{"provider":{"ollama":{"options":{"baseURL":"https://remote.example.test/v1"}}}}"#,
    )
    .unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");
    let models = vec!["ollama/llama3.2".to_owned()];
    assert!(matches!(
        probe_local_models(&models),
        LocalModelEvidence::NotConfigured
    ));

    std::fs::write(
        &config_path,
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1"}}}}"#,
    )
    .unwrap();
    environment.set("OPENCODE_CONFIG", config_path.as_os_str());
    assert!(matches!(
        probe_local_models(&models),
        LocalModelEvidence::NotConfigured
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remote_project_ollama_override_is_not_classified_as_a_local_model() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-project-config-{}",
        uuid::Uuid::new_v4()
    ));
    let project = root.join("workspace");
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1"}}}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join("opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"https://remote.example.test/v1"}}}}"#,
    )
    .unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");

    assert_eq!(
        probe_local_models_in(&["ollama/llama3.2".to_owned()], &project),
        LocalModelEvidence::NotConfigured,
        "a project config can change the endpoint used by discovery"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remote_nested_opencode_project_override_is_not_classified_as_local() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-nested-project-config-{}",
        uuid::Uuid::new_v4()
    ));
    let project = root.join("workspace");
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    std::fs::create_dir_all(project.join(".opencode")).unwrap();
    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1"}}}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".opencode/opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"https://remote.example.test/v1"}}}}"#,
    )
    .unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");

    assert_eq!(
        probe_local_models_in(&["ollama/llama3.2".to_owned()], &project),
        LocalModelEvidence::NotConfigured,
        "nested project config can change the endpoint used by discovery"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_without_an_endpoint_override_is_ambiguous_when_configured() {
    let config = parse_jsonc(r#"{"provider":{"ollama":{"models":{}}}}"#).unwrap();
    let mut endpoints = BTreeMap::new();
    merge_endpoints(&mut endpoints, &config).unwrap();
    assert!(endpoints.contains_key("ollama"));
    assert!(!endpoints.get("ollama").unwrap().loopback_configured);
    assert!(!endpoints.get("ollama").unwrap().no_auth_required);
}

#[test]
/// Production boundary: an explicitly configured no-auth loopback provider is probed over TCP,
/// and an unavailable service remains model evidence rather than an auth failure. Pure URL tests
/// cannot prove the bounded connection attempt and typed adapter result. External dependency: none;
/// setup cost: bind and close one ephemeral local port, then make one bounded connection attempt.
fn explicit_local_provider_configuration_separates_auth_from_unavailable_catalog() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-local-unavailable-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        format!(r#"{{"provider":{{"ollama":{{"options":{{"baseURL":"http://{address}/v1"}}}}}}}}"#),
    )
    .unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");

    assert!(matches!(
        probe_local_models_in(&["ollama/llama3.2".to_owned()], &root),
        LocalModelEvidence::EndpointUnavailable
    ));
    std::fs::remove_dir_all(root).unwrap();
}
