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

/// Materialize a miniature, layout-realistic Pi package inside the fixture:
/// `<root>/package.json` + `<root>/dist/bundle/cli.js` (the shape Pi's own
/// package-root walk keys on).
fn fake_pi_executable(controller: &Fixture) -> PathBuf {
    let root = controller.0.join("pi-package");
    fs::create_dir_all(root.join("dist").join("bundle")).unwrap();
    fs::write(
        root.join("package.json"),
        br#"{"name":"@earendil-works/pi-coding-agent"}"#,
    )
    .unwrap();
    let executable = root.join("dist").join("bundle").join("cli.js");
    fs::write(&executable, b"// stub\n").unwrap();
    executable
}

#[test]
fn stages_only_the_selected_model_provider_and_authentication() {
    let source = source_config();
    let controller = Fixture::new();
    let profile = prepare_from(
        &source.0,
        &controller.0,
        Some("model-2"),
        &fake_pi_executable(&controller),
    )
    .unwrap();

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
    let profile = prepare_from(
        &source.0,
        &controller.0,
        None,
        &fake_pi_executable(&controller),
    )
    .unwrap();
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

    assert!(
        prepare_from(
            &source.0,
            &controller.0,
            None,
            &fake_pi_executable(&controller)
        )
        .is_err()
    );
}

#[test]
fn bridges_builtin_assets_of_the_owning_pi_package() {
    let source = source_config();
    let controller = Fixture::new();
    let executable = fake_pi_executable(&controller);
    let profile = prepare_from(&source.0, &controller.0, None, &executable).unwrap();

    let linked_dist = profile.package_directory.join("dist");
    #[cfg(unix)]
    {
        assert!(linked_dist.is_symlink());
        // The bridge must point at the owning package's real `dist`.
        assert_eq!(
            fs::read_link(&linked_dist).unwrap(),
            controller.0.join("pi-package").join("dist")
        );
    }
    #[cfg(windows)]
    assert!(
        linked_dist.is_dir(),
        "Windows must bridge dist without symlink privilege"
    );
    assert!(linked_dist.join("bundle/cli.js").exists());
    assert!(profile.package_directory.join("package.json").exists());
    // The asset bridge must not expose configuration or credential surfaces.
    assert!(!profile.package_directory.join("settings.json").exists());
    assert!(!profile.package_directory.join("auth.json").exists());
    assert!(!profile.package_directory.join("models.json").exists());
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_pi_package_metadata() {
    use std::os::unix::fs::symlink;

    let source = source_config();
    let controller = Fixture::new();
    let executable = fake_pi_executable(&controller);
    let root = controller.0.join("pi-package");
    let external = controller.0.join("external-package.json");
    fs::write(&external, br#"{"name":"@earendil-works/pi-coding-agent"}"#).unwrap();
    fs::remove_file(root.join("package.json")).unwrap();
    symlink(external, root.join("package.json")).unwrap();

    let error = prepare_from(&source.0, &controller.0, None, &executable).unwrap_err();
    assert!(error.to_string().contains("bounded regular file"));
}

#[test]
fn refuses_launch_when_no_pi_package_can_be_located() {
    let source = source_config();
    let controller = Fixture::new();
    fs::create_dir_all(controller.0.join("loose-bin")).unwrap();
    let orphan = controller.0.join("loose-bin").join("pi");
    fs::write(&orphan, b"#!/usr/bin/env node\n").unwrap();
    let error = prepare_from(&source.0, &controller.0, None, &orphan).unwrap_err();
    assert!(
        error.to_string().contains("Cannot locate the Pi package"),
        "unexpected refusal reason: {error}"
    );
}

#[test]
fn refuses_assets_from_a_package_with_the_wrong_identity() {
    let source = source_config();
    let controller = Fixture::new();
    let executable = fake_pi_executable(&controller);
    fs::write(
        controller.0.join("pi-package/package.json"),
        r#"{"name":"untrusted-package"}"#,
    )
    .unwrap();

    let error = prepare_from(&source.0, &controller.0, None, &executable).unwrap_err();
    assert!(
        error.to_string().contains("failed the identity check"),
        "unexpected refusal reason: {error}"
    );
}
