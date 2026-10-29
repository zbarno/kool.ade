use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};

mod asset_bridge;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub(super) struct CleanPiProfile {
    pub agent_directory: PathBuf,
    pub session_directory: PathBuf,
    pub package_directory: PathBuf,
    pub provider: String,
    pub model: String,
}

pub(super) fn prepare(
    controller_directory: &Path,
    model_override: Option<&str>,
    pi_executable: &Path,
) -> anyhow::Result<CleanPiProfile> {
    let source = configured_agent_directory()?;
    prepare_from(&source, controller_directory, model_override, pi_executable)
}

fn prepare_from(
    source: &Path,
    controller_directory: &Path,
    model_override: Option<&str>,
    pi_executable: &Path,
) -> anyhow::Result<CleanPiProfile> {
    let metadata = fs::symlink_metadata(source)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Pi configuration directory must be a real directory"
    );
    let settings = read_json(&source.join("settings.json"))?;
    let models = read_json(&source.join("models.json"))?;
    let provider = settings["defaultProvider"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Pi has no configured default provider"))?;
    let default_model = settings["defaultModel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Pi has no configured default model"))?;
    let provider_config = &models["providers"][provider];
    let available_models = provider_config["models"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Pi's selected provider has no model catalog"))?;
    let selected_model = model_override.unwrap_or(default_model);
    let model_config = available_models
        .iter()
        .find(|candidate| candidate["id"].as_str() == Some(selected_model))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Selected Pi model '{selected_model}' is absent from the selected provider's catalog"
            )
        })?;

    let agent_directory = controller_directory.join("pi-agent");
    create_private_directory(&agent_directory)?;
    let session_directory = controller_directory.join("pi-sessions");
    create_private_directory(&session_directory)?;
    let package_directory = controller_directory.join("pi-packages");
    create_private_directory(&package_directory)?;
    asset_bridge::link_builtin_package_assets(&package_directory, pi_executable)?;
    let clean_settings = json!({
        "defaultProvider": provider,
        "defaultModel": selected_model,
    });
    let mut clean_provider = copy_fields(
        provider_config,
        &["api", "baseUrl", "apiKey", "headers", "compat"],
    );
    clean_provider
        .as_object_mut()
        .expect("provider object was created from an object")
        .insert(
            "models".into(),
            Value::Array(vec![copy_fields(
                model_config,
                &[
                    "id",
                    "name",
                    "api",
                    "baseUrl",
                    "reasoning",
                    "input",
                    "cost",
                    "contextWindow",
                    "maxTokens",
                    "headers",
                    "compat",
                ],
            )]),
        );
    let mut provider_entries = Map::new();
    provider_entries.insert(provider.to_owned(), clean_provider);
    let clean_models = json!({ "providers": provider_entries });
    write_json(&agent_directory.join("settings.json"), &clean_settings)?;
    write_json(&agent_directory.join("models.json"), &clean_models)?;

    let auth_path = source.join("auth.json");
    match read_optional_json(&auth_path)? {
        Some(auth) if auth.get(provider).is_some() => {
            let provider_auth = auth[provider].clone();
            let mut provider_entries = Map::new();
            provider_entries.insert(provider.to_owned(), provider_auth);
            write_json(
                &agent_directory.join("auth.json"),
                &Value::Object(provider_entries),
            )?;
        }
        Some(_) | None => {}
    }

    Ok(CleanPiProfile {
        agent_directory,
        session_directory,
        package_directory,
        provider: provider.to_owned(),
        model: selected_model.to_owned(),
    })
}

fn configured_agent_directory() -> anyhow::Result<PathBuf> {
    // Share the probe/profile-directory resolver so the seeded profile and
    // the readiness probe provably inspect the same Pi profile source.
    let path = crate::harness::pi_provider_settings::agent_directory()
        .ok_or_else(|| anyhow::anyhow!("Pi configuration directory is unavailable"))?;
    anyhow::ensure!(
        path.is_absolute(),
        "Pi configuration directory must be absolute"
    );
    Ok(path)
}

fn read_optional_json(path: &Path) -> anyhow::Result<Option<Value>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= MAX_CONFIG_BYTES,
        "Pi configuration file is not a bounded regular file"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "Pi configuration file exceeds the size limit"
    );
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    read_optional_json(path)?.ok_or_else(|| anyhow::anyhow!("Pi configuration file is missing"))
}

fn copy_fields(source: &Value, names: &[&str]) -> Value {
    let mut output = Map::new();
    for name in names {
        if let Some(value) = source.get(*name) {
            output.insert((*name).to_owned(), value.clone());
        }
    }
    Value::Object(output)
}

fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(path)?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(path)?.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
#[path = "clean_profile_tests.rs"]
mod tests;
