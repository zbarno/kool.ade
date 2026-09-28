use super::Target;
use serde_json::{Map, Value, json};
use std::{
    fs,
    net::{IpAddr, TcpListener, ToSocketAddrs},
    path::Path,
};

mod validate;
use validate::{ensure_supported_api, validate_base_url_shape};

pub(super) struct Config {
    pub(super) target: Target,
    pub(super) key: String,
    pub(super) port: u16,
    pub(super) model_args: Vec<String>,
    models: Value,
    settings: Value,
}

pub(super) fn load() -> anyhow::Result<Config> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Cannot find Pi home for planning provider"))?;
    load_from(&home.join(".pi/agent"))
}

pub(super) fn provider_error() -> Option<String> {
    if let Some(error) = validate::configuration_error() {
        return Some(error);
    }
    let home = match std::env::var_os("HOME") {
        Some(home) => std::path::PathBuf::from(home),
        None => return Some("Cannot find Pi home for planning provider".into()),
    };
    load_from(&home.join(".pi/agent"))
        .err()
        .map(|error| error.to_string())
}

fn load_from(agent: &Path) -> anyhow::Result<Config> {
    let settings: Value = read_json(&agent.join("settings.json"))?;
    let model_store: Value = read_json(&agent.join("models.json"))?;
    let provider = settings["defaultProvider"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Pi has no default model provider"))?
        .to_owned();
    let model_id = settings["defaultModel"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Pi has no default model"))?
        .to_owned();
    let definition = &model_store["providers"][&provider];
    ensure_supported_api(definition["api"].as_str())?;
    let target = parse_target(
        definition["baseUrl"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Pi model provider has no configured base URL"))?,
    )?;
    let auth: Value = read_json(&agent.join("auth.json"))?;
    let key = auth[&provider]["key"]
        .as_str()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Pi has no saved API key for its selected provider"))?
        .to_owned();
    let model = definition["models"]
        .as_array()
        .and_then(|models| {
            models
                .iter()
                .find(|model| model["id"].as_str() == Some(&model_id))
        })
        .ok_or_else(|| anyhow::anyhow!("Pi's selected model is absent from its provider config"))?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let sanitized_url = format!("http://127.0.0.1:{port}{}", target.prefix);
    let model = copy_fields(
        model,
        &[
            "id",
            "name",
            "reasoning",
            "input",
            "cost",
            "contextWindow",
            "maxTokens",
        ],
    );
    let provider_config = json!({
        "api": "openai-completions",
        "baseUrl": sanitized_url,
        "apiKey": "packet-proxy",
        "compat": copy_fields(&definition["compat"], &[
            "supportsDeveloperRole", "supportsReasoningEffort", "thinkingFormat"
        ]),
        "models": [model]
    });
    let mut providers = Map::new();
    providers.insert(provider.clone(), provider_config);
    Ok(Config {
        target,
        key,
        port,
        model_args: vec![
            "--provider".into(),
            provider.clone(),
            "--model".into(),
            model_id.clone(),
            "--api-key".into(),
            "packet-proxy".into(),
        ],
        models: json!({"providers": providers}),
        settings: json!({"defaultProvider": provider, "defaultModel": model_id}),
    })
}

impl Config {
    pub(super) fn write_sandbox_files(&self, agent: &Path) -> anyhow::Result<()> {
        use std::io::Write;
        for (name, content) in [
            ("settings.json", serde_json::to_vec(&self.settings)?),
            ("models.json", serde_json::to_vec(&self.models)?),
        ] {
            let mut file = fs::File::create(agent.join(name))?;
            file.write_all(&content)?;
        }
        fs::write(agent.join("model-relay.cjs"), super::SANDBOX_RELAY)?;
        Ok(())
    }
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    let bytes = fs::read(path)
        .map_err(|error| anyhow::anyhow!("Cannot load Pi provider configuration: {error}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn copy_fields(source: &Value, names: &[&str]) -> Value {
    let mut output = Map::new();
    for name in names {
        if let Some(value) = source.get(name) {
            output.insert((*name).into(), value.clone());
        }
    }
    Value::Object(output)
}

fn parse_target(base_url: &str) -> anyhow::Result<Target> {
    validate_base_url_shape(base_url)?;
    let rest = base_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("Planning provider must use a bounded HTTP endpoint"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{}", path.trim_end_matches('/'));
    let prefix = format!("{path}/");
    let (host, port) = authority
        .rsplit_once(':')
        .map(|(host, port)| {
            port.parse::<u16>()
                .map(|port| (host, port))
                .map_err(|_| anyhow::anyhow!("Planning provider URL has an invalid port"))
        })
        .transpose()?
        .unwrap_or((authority, 80));
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| {
            anyhow::anyhow!("Cannot resolve configured local planning provider: {error}")
        })?
        .collect::<Vec<_>>();
    let address = addresses
        .into_iter()
        .find(|address| local_target(address.ip()))
        .ok_or_else(|| {
            anyhow::anyhow!("Planning provider must resolve to a private/local address")
        })?;
    Ok(Target {
        address,
        host: authority.to_owned(),
        prefix,
    })
}

fn local_target(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| local_target(mapped.into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_provider_configuration_names_supported_protocol() {
        let root = std::env::temp_dir().join(format!(
            "packet_provider_config_unsupported_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("settings.json"),
            r#"{"defaultProvider":"hosted","defaultModel":"model"}"#,
        )
        .unwrap();
        fs::write(
            root.join("models.json"),
            r#"{"providers":{"hosted":{"api":"anthropic-messages"}}}"#,
        )
        .unwrap();
        let error = match load_from(&root) {
            Ok(_) => panic!("unsupported provider configuration must fail closed"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("only OpenAI-compatible local providers"),
            "unexpected provider setup error: {error}"
        );
        assert_eq!(
            validate::configuration_error_from(&root).as_deref(),
            Some(error.as_str()),
            "the connection setup check must report the same unsupported-provider cause"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_rejects_public_and_credential_bearing_urls() {
        assert!(parse_target("http://127.0.0.1:8000/v1/").is_ok());
        assert!(parse_target("http://user:password@127.0.0.1:8000/v1/").is_err());
        assert!(parse_target("https://127.0.0.1:8000/v1/").is_err());
        assert!(parse_target("http://example.com/v1/").is_err());
    }

    #[test]
    fn private_provider_credential_is_not_written_to_sandbox_configuration() {
        let root = std::env::temp_dir().join(format!(
            "packet_provider_config_test_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir(&root).unwrap();
        let config = Config {
            target: Target {
                address: "127.0.0.1:8000".parse().unwrap(),
                host: "127.0.0.1:8000".into(),
                prefix: "/v1/".into(),
            },
            key: "private-test-secret".into(),
            port: 43210,
            model_args: vec!["--api-key".into(), "packet-proxy".into()],
            models: json!({"providers":{"test":{"apiKey":"packet-proxy"}}}),
            settings: json!({"defaultProvider":"test","defaultModel":"model"}),
        };
        config.write_sandbox_files(&root).unwrap();
        for entry in fs::read_dir(&root).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("private-test-secret"));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
