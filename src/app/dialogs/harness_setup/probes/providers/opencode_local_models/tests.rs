use super::managed::has_managed_config;
use super::*;

#[test]
fn local_endpoint_is_loopback_only() {
    for endpoint in [
        "http://localhost:11434/v1",
        "http://127.0.0.1:1234/v1",
        "https://[::1]:8000/v1",
    ] {
        assert!(is_loopback_endpoint(endpoint), "{endpoint}");
    }
    for endpoint in [
        "https://api.example.test/v1",
        "http://localhost.example.test/v1",
        "file:///tmp/model",
        "not a url",
    ] {
        assert!(!is_loopback_endpoint(endpoint), "{endpoint}");
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
    assert_eq!(endpoints.get("custom"), Some(&true));
    merge_endpoints(&mut endpoints, &remote).unwrap();
    assert_eq!(endpoints.get("custom"), Some(&false));
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
    assert_eq!(models_available_without_authentication(&models), None);

    std::fs::write(
        &config_path,
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1"}}}}"#,
    )
    .unwrap();
    environment.set("OPENCODE_CONFIG", config_path.as_os_str());
    assert_eq!(models_available_without_authentication(&models), None);
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
        models_available_without_authentication_in(&["ollama/llama3.2".to_owned()], &project),
        None,
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
        models_available_without_authentication_in(&["ollama/llama3.2".to_owned()], &project),
        None,
        "nested project config can change the endpoint used by discovery"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_without_an_endpoint_override_is_ambiguous_when_configured() {
    let config = parse_jsonc(r#"{"provider":{"ollama":{"models":{}}}}"#).unwrap();
    let mut endpoints = BTreeMap::new();
    merge_endpoints(&mut endpoints, &config).unwrap();
    assert_eq!(endpoints.get("ollama"), Some(&false));
}
