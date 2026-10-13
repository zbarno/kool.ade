use super::prepare_from;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("koolade-pi-clean-profile-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn config(&self, name: &str, value: &Value) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source_config() -> Fixture {
    let fixture = Fixture::new();
    fixture.config(
        "settings.json",
        &json!({
            "defaultProvider": "provider-a",
            "defaultModel": "model-1",
            "extensions": ["untrusted-extension"],
            "skills": ["untrusted-skill"],
            "theme": "custom-theme",
            "mcpServers": { "untrusted": { "command": "hook" } }
        }),
    );
    fixture.config(
        "models.json",
        &json!({
            "providers": {
                "provider-a": {
                    "api": "openai-completions",
                    "baseUrl": "https://api.example.invalid/v1",
                    "apiKey": "selected-provider-key",
                    "headers": { "x-test": "yes" },
                    "models": [
                        { "id": "model-1", "name": "Model One", "contextWindow": 1000 },
                        { "id": "model-2", "name": "Model Two", "contextWindow": 2000 }
                    ]
                },
                "provider-b": { "apiKey": "unselected-provider-key" }
            }
        }),
    );
    fixture.config(
        "auth.json",
        &json!({
            "provider-a": { "key": "selected-login" },
            "provider-b": { "key": "unselected-login" }
        }),
    );
    fixture
}

#[test]
fn stages_only_the_selected_model_provider_and_authentication() {
    let source = source_config();
    let controller = Fixture::new();
    let profile = prepare_from(&source.0, &controller.0, Some("model-2")).unwrap();

    assert_eq!(profile.provider, "provider-a");
    assert_eq!(profile.model, "model-2");
    let settings: Value =
        serde_json::from_slice(&fs::read(profile.agent_directory.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings,
        json!({"defaultProvider":"provider-a","defaultModel":"model-2"})
    );

    let models: Value =
        serde_json::from_slice(&fs::read(profile.agent_directory.join("models.json")).unwrap())
            .unwrap();
    assert!(models["providers"]["provider-b"].is_null());
    assert_eq!(
        models["providers"]["provider-a"]["models"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        models["providers"]["provider-a"]["models"][0]["id"],
        "model-2"
    );
    assert!(models["providers"]["provider-a"]["extensions"].is_null());

    let auth: Value =
        serde_json::from_slice(&fs::read(profile.agent_directory.join("auth.json")).unwrap())
            .unwrap();
    assert_eq!(auth["provider-a"]["key"], "selected-login");
    assert!(auth["provider-b"].is_null());
}

#[cfg(unix)]
#[test]
fn staged_pi_credentials_and_configuration_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let source = source_config();
    let controller = Fixture::new();
    let profile = prepare_from(&source.0, &controller.0, None).unwrap();
    assert_eq!(
        profile
            .agent_directory
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for name in ["settings.json", "models.json", "auth.json"] {
        assert_eq!(
            profile
                .agent_directory
                .join(name)
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_user_configuration_is_rejected() {
    use std::os::unix::fs::symlink;

    let source = source_config();
    let controller = Fixture::new();
    let outside = controller.0.join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    fs::remove_file(source.0.join("auth.json")).unwrap();
    symlink(&outside, source.0.join("auth.json")).unwrap();

    assert!(prepare_from(&source.0, &controller.0, None).is_err());
}
