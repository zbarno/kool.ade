use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{catalog::explicit_loopback_catalog_url, managed::has_platform_managed_config};

const CONFIG_LIMIT: u64 = 1_048_576;
const NO_AUTH_LOCAL_PROVIDERS: [&str; 3] = ["lmstudio", "ollama", "vllm"];

pub(super) struct ProviderEndpoint {
    pub(super) loopback_configured: bool,
    pub(super) catalog_url: Option<url::Url>,
    pub(super) no_auth_required: bool,
}

pub(super) fn effective_provider_endpoints() -> Option<BTreeMap<String, ProviderEndpoint>> {
    if has_platform_managed_config()? {
        return None;
    }
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let paths = [
        config_home.join("opencode/opencode.json"),
        config_home.join("opencode/opencode.jsonc"),
    ];
    let mut endpoints = BTreeMap::new();
    for path in paths {
        if let Some(config) = read_config(&path)? {
            merge_endpoints(&mut endpoints, &config)?;
        }
    }
    Some(endpoints)
}

pub(super) fn has_project_config(directory: &Path) -> Option<bool> {
    for directory in directory.ancestors() {
        for path in [
            directory.join("opencode.json"),
            directory.join("opencode.jsonc"),
            directory.join(".opencode/opencode.json"),
            directory.join(".opencode/opencode.jsonc"),
        ] {
            match fs::metadata(path) {
                Ok(_) => return Some(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return None,
            }
        }
    }
    Some(false)
}

fn read_config(path: &Path) -> Option<Option<Value>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    if !metadata.is_file() || metadata.len() > CONFIG_LIMIT {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(CONFIG_LIMIT + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > CONFIG_LIMIT {
        return None;
    }
    parse_jsonc(&text).map(Some)
}

pub(super) fn merge_endpoints(
    endpoints: &mut BTreeMap<String, ProviderEndpoint>,
    config: &Value,
) -> Option<()> {
    let config = config.as_object()?;
    let Some(providers) = config.get("provider") else {
        return Some(());
    };
    let providers = providers.as_object()?;
    for (provider_id, provider) in providers {
        let provider = provider.as_object()?;
        let options = provider.get("options").and_then(Value::as_object);
        let base_url = options
            .and_then(|options| options.get("baseURL"))
            .and_then(Value::as_str);
        let loopback_configured = base_url
            .and_then(super::catalog::configured_loopback_url)
            .is_some();
        let catalog_url = base_url.and_then(explicit_loopback_catalog_url);
        let has_auth_override = provider.contains_key("headers")
            || provider.contains_key("apiKey")
            || options.is_some_and(|options| {
                options.contains_key("headers") || options.contains_key("apiKey")
            });
        endpoints.insert(
            provider_id.clone(),
            ProviderEndpoint {
                loopback_configured,
                catalog_url,
                no_auth_required: loopback_configured
                    && !has_auth_override
                    && NO_AUTH_LOCAL_PROVIDERS.contains(&provider_id.as_str()),
            },
        );
    }
    Some(())
}

pub(super) fn parse_jsonc(text: &str) -> Option<Value> {
    serde_json::from_str(&strip_jsonc(text)).ok()
}

fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
        } else if byte == b'"' {
            output.push(byte);
            in_string = true;
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                if bytes[index] == b'\n' {
                    output.push(b'\n');
                }
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else {
            output.push(byte);
            index += 1;
        }
    }
    let mut clean = Vec::with_capacity(output.len());
    let mut index = 0;
    in_string = false;
    escaped = false;
    while index < output.len() {
        let byte = output[index];
        if in_string {
            clean.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
        } else if byte == b'"' {
            clean.push(byte);
            in_string = true;
            index += 1;
        } else if byte == b',' {
            let next = output[index + 1..]
                .iter()
                .copied()
                .find(|byte| !byte.is_ascii_whitespace());
            if !matches!(next, Some(b'}' | b']')) {
                clean.push(byte);
            }
            index += 1;
        } else {
            clean.push(byte);
            index += 1;
        }
    }
    String::from_utf8(clean).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::parse_jsonc;

    #[test]
    fn jsonc_parser_ignores_comments_and_trailing_commas() {
        let value = parse_jsonc(
            r#"{
                // comment
                "provider": { "ollama": { "models": {}, }, },
            }"#,
        )
        .unwrap();
        assert!(value["provider"]["ollama"]["models"].is_object());
    }
}
