use std::{path::Path, time::Duration};

use zeroize::{Zeroize, Zeroizing};

const EXPORT_TIMEOUT: Duration = Duration::from_secs(8);
const EXPORT_OUTPUT_LIMIT: usize = 64 * 1024;

pub(crate) enum AuthExport {
    ApiKey(Zeroizing<String>),
    NoSavedCredential,
    Unsupported,
}

/// Reads only the selected integration's saved API key through OpenCode's documented export.
/// Captured output is private, bounded, and zeroized; it is never copied to diagnostics.
pub(crate) fn export_api_key(binary: &Path, integration: &str) -> AuthExport {
    let argv = [
        binary.to_string_lossy().into_owned(),
        "--pure".into(),
        "auth".into(),
        "export".into(),
        integration.into(),
    ];
    let data_home = match std::env::var_os("XDG_DATA_HOME") {
        Some(value) => match value.to_str() {
            Some(value) => Some(value.to_owned()),
            None => return AuthExport::Unsupported,
        },
        None => None,
    };
    let overrides = data_home.as_deref().map(|value| [("XDG_DATA_HOME", value)]);
    let output = match overrides.as_ref() {
        Some(overrides) => crate::harness::discovery::run_with_environment(
            &argv,
            EXPORT_TIMEOUT,
            EXPORT_OUTPUT_LIMIT,
            EXPORT_OUTPUT_LIMIT,
            crate::harness::execution_security::CliProvider::OpenCode.excluded_child_environment(),
            overrides,
        ),
        None => crate::harness::discovery::run(
            &argv,
            EXPORT_TIMEOUT,
            EXPORT_OUTPUT_LIMIT,
            EXPORT_OUTPUT_LIMIT,
            crate::harness::execution_security::CliProvider::OpenCode.excluded_child_environment(),
        ),
    };
    let Ok(mut output) = output else {
        return AuthExport::Unsupported;
    };
    if !output.success || output.stdout_truncated || output.stderr_truncated {
        output.stdout.zeroize();
        output.stderr.zeroize();
        return AuthExport::Unsupported;
    }
    let result = parse_export(&output.stdout, integration);
    output.stdout.zeroize();
    output.stderr.zeroize();
    result
}

fn parse_export(bytes: &[u8], integration: &str) -> AuthExport {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return AuthExport::Unsupported;
    };
    let mut value = SensitiveJson(value);
    let Some(entries) = value.0.as_array_mut() else {
        return AuthExport::Unsupported;
    };
    if entries.is_empty() {
        return AuthExport::NoSavedCredential;
    }

    let mut active_index = None;
    for (index, entry) in entries.iter().enumerate() {
        if entry
            .get("integrationID")
            .and_then(serde_json::Value::as_str)
            != Some(integration)
        {
            return AuthExport::Unsupported;
        }
        if entry.get("active").and_then(serde_json::Value::as_bool) == Some(true)
            && active_index.replace(index).is_some()
        {
            return AuthExport::Unsupported;
        }
    }
    let Some(index) = active_index else {
        return AuthExport::Unsupported;
    };
    let Some(auth) = entries[index]
        .get_mut("value")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return AuthExport::Unsupported;
    };
    if auth.get("type").and_then(serde_json::Value::as_str) != Some("key") {
        return AuthExport::Unsupported;
    }
    let Some(serde_json::Value::String(mut key)) = auth.remove("key") else {
        return AuthExport::Unsupported;
    };
    if key.len() < 8 || key.len() > 8192 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        key.zeroize();
        return AuthExport::Unsupported;
    }
    AuthExport::ApiKey(Zeroizing::new(key))
}

struct SensitiveJson(serde_json::Value);

impl Drop for SensitiveJson {
    fn drop(&mut self) {
        zeroize_json(&mut self.0);
    }
}

fn zeroize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(zeroize_json),
        serde_json::Value::Object(values) => {
            for (mut key, mut value) in std::mem::take(values) {
                key.zeroize();
                zeroize_json(&mut value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
