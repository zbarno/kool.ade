use serde_json::{Map, Value, json};
use std::{fs, io::Read, path::Path};

const CONFIG_LIMIT: u64 = 1_048_576;

pub(super) fn provider_settings() -> anyhow::Result<Value> {
    for path in [
        Path::new("/etc/opencode/opencode.json"),
        Path::new("/etc/opencode/opencode.jsonc"),
    ] {
        anyhow::ensure!(
            !path.exists(),
            "OpenCode has administrator-managed settings; Kool.ad/e cannot verify its tool policy"
        );
    }
    let mut output = json!({
        "permission": {},
        "tools": {},
        "agent": {},
        "mcp": {},
        "provider": {},
    });
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(Into::into)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")));
    if let Some(config_home) = config_home {
        for name in ["opencode.json", "opencode.jsonc"] {
            merge_provider_values(
                &mut output,
                read_config(&config_home.join("opencode").join(name))?,
            );
        }
    }
    if let Some(custom) = std::env::var_os("OPENCODE_CONFIG") {
        merge_provider_values(&mut output, read_config(Path::new(&custom))?);
    }
    if let Ok(content) = std::env::var("OPENCODE_CONFIG_CONTENT") {
        merge_provider_values(&mut output, parse_config(&content)?);
    }
    Ok(output)
}

pub(super) fn copy_auth(
    boundary: &super::super::super::ApplicationBoundary,
    home: &Path,
) -> anyhow::Result<()> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(Into::into)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")));
    let Some(source) = data_home.map(|root| root.join("opencode/auth.json")) else {
        return Ok(());
    };
    if !source.exists() {
        return Ok(());
    }
    let value = read_config(&source)?;
    let destination = home.join(".local/share/opencode/auth.json");
    let relative = destination.strip_prefix(&boundary.scratch.path)?;
    boundary.scratch.write_json(
        relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("OpenCode auth path is not valid UTF-8"))?,
        &value,
    )?;
    Ok(())
}

fn read_config(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(Value::Null);
    }
    let metadata = fs::metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= CONFIG_LIMIT,
        "OpenCode configuration exceeds the private import limit"
    );
    let mut content = String::new();
    fs::File::open(path)?
        .take(CONFIG_LIMIT + 1)
        .read_to_string(&mut content)?;
    anyhow::ensure!(
        content.len() as u64 <= CONFIG_LIMIT,
        "OpenCode configuration exceeds the private import limit"
    );
    parse_config(&content)
}

fn parse_config(text: &str) -> anyhow::Result<Value> {
    let stripped = strip_jsonc(text);
    let value: Value = serde_json::from_str(&stripped)?;
    anyhow::ensure!(
        value.is_object() || value.is_null(),
        "OpenCode configuration must be a JSON object"
    );
    Ok(value)
}

fn merge_provider_values(output: &mut Value, input: Value) {
    let Some(input) = input.as_object() else {
        return;
    };
    for key in ["model", "small_model"] {
        if let Some(value) = input.get(key).filter(|value| value.is_string()) {
            output[key] = value.clone();
        }
    }
    let Some(providers) = input.get("provider").and_then(Value::as_object) else {
        return;
    };
    let destination = output["provider"].as_object_mut().expect("provider object");
    for (name, provider) in providers {
        if let Some(provider) = provider.as_object() {
            let mut provider = provider.clone();
            provider.remove("npm");
            merge_object(
                destination.entry(name.clone()).or_insert_with(|| json!({})),
                provider,
            );
        }
    }
}

fn merge_object(destination: &mut Value, source: Map<String, Value>) {
    let Some(destination) = destination.as_object_mut() else {
        return;
    };
    for (key, value) in source {
        destination.insert(key, value);
    }
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
            in_string = true;
            output.push(byte);
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
    let mut without_trailing_commas = Vec::with_capacity(output.len());
    let mut index = 0;
    in_string = false;
    escaped = false;
    while index < output.len() {
        let byte = output[index];
        if in_string {
            without_trailing_commas.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
        } else if byte == b'"' {
            in_string = true;
            without_trailing_commas.push(byte);
            index += 1;
        } else if byte == b',' {
            let next = output[index + 1..]
                .iter()
                .copied()
                .find(|byte| !byte.is_ascii_whitespace());
            if !matches!(next, Some(b'}' | b']')) {
                without_trailing_commas.push(byte);
            }
            index += 1;
        } else {
            without_trailing_commas.push(byte);
            index += 1;
        }
    }
    String::from_utf8(without_trailing_commas).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_parser_preserves_comment_markers_in_strings_and_strips_trailing_commas() {
        let config = parse_config(
            r#"{ // comment
            "model": "provider/model//v1",
            "provider": { "custom": { "npm": "unsafe", "options": {}, }, },
        }"#,
        )
        .unwrap();
        assert_eq!(config["model"], "provider/model//v1");
        let mut output = json!({ "provider": {} });
        merge_provider_values(&mut output, config);
        assert!(output["provider"]["custom"].get("npm").is_none());
    }
}
