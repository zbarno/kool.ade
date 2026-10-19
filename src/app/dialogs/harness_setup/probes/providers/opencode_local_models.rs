use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

mod managed;
use managed::has_platform_managed_config;

const CONFIG_LIMIT: u64 = 1_048_576;
const BUILTIN_LOCAL_PROVIDERS: [&str; 3] = ["lmstudio", "ollama", "vllm"];

/// Treat a model as local only when its effective configured endpoint is loopback. Built-in
/// local providers without an override use their documented local default; any provider config
/// override must explicitly prove a loopback base URL. Environment config overrides are rejected
/// because discovery runs with those variables sanitized.
pub(super) fn models_available_without_authentication(models: &[String]) -> Option<Vec<String>> {
    models_available_without_authentication_in(models, &std::env::current_dir().ok()?)
}

fn models_available_without_authentication_in(
    models: &[String],
    discovery_directory: &Path,
) -> Option<Vec<String>> {
    if std::env::var_os("OPENCODE_CONFIG").is_some()
        || std::env::var_os("OPENCODE_CONFIG_CONTENT").is_some()
    {
        return None;
    }
    // `opencode --pure models` still reads project config from its inherited cwd. Runtime
    // execution uses a private `--dir`, so a project override cannot be safely reconciled here.
    if has_project_config(discovery_directory)? {
        return None;
    }
    let endpoints = effective_provider_endpoints()?;
    let local = models
        .iter()
        .filter(|model| {
            let Some((provider, model_id)) = model.split_once('/') else {
                return false;
            };
            if model_id.trim().is_empty() {
                return false;
            }
            match endpoints.get(provider) {
                Some(is_local) => *is_local,
                None => BUILTIN_LOCAL_PROVIDERS.contains(&provider),
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    (!local.is_empty()).then_some(local)
}

fn has_project_config(directory: &Path) -> Option<bool> {
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

fn effective_provider_endpoints() -> Option<BTreeMap<String, bool>> {
    // System-wide configuration is rejected by the execution adapter, so do not infer locality
    // from a potentially administrator-managed override during discovery.
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

fn merge_endpoints(endpoints: &mut BTreeMap<String, bool>, config: &Value) -> Option<()> {
    let config = config.as_object()?;
    let Some(providers) = config.get("provider") else {
        return Some(());
    };
    let providers = providers.as_object()?;
    for (provider_id, provider) in providers {
        let provider = provider.as_object()?;
        let Some(options) = provider.get("options") else {
            endpoints.insert(provider_id.clone(), false);
            continue;
        };
        let options = options.as_object()?;
        let Some(base_url) = options.get("baseURL") else {
            endpoints.insert(provider_id.clone(), false);
            continue;
        };
        let local = base_url.as_str().is_some_and(is_loopback_endpoint);
        endpoints.insert(provider_id.clone(), local);
    }
    Some(())
}

fn is_loopback_endpoint(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn parse_jsonc(text: &str) -> Option<Value> {
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
#[path = "opencode_local_models/tests.rs"]
mod tests;
