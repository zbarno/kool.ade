use serde_json::Value;
use std::path::Path;

pub(super) fn fetch(
    _config: &super::ServerConfig,
    _args: &Value,
) -> Result<(String, bool), String> {
    Err("Docker resource tools are unavailable on Windows".into())
}

pub(super) fn dependency(
    _config_path: &Path,
    _config: &super::ServerConfig,
    _args: &Value,
) -> Result<(String, bool), String> {
    Err("Docker dependency tools are unavailable on Windows".into())
}
