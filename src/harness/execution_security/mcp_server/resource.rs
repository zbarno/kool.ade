use super::{ServerConfig, sandbox};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
};

pub(super) fn fetch(config: &ServerConfig, args: &Value) -> Result<(String, bool), String> {
    let Some(url) = args["url"].as_str().filter(|value| value.len() <= 4096) else {
        return Err("A valid HTTPS URL is required".into());
    };
    let Some(purpose) = args["purpose"]
        .as_str()
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
    else {
        return Err("A short resource purpose is required".into());
    };
    let response = request(
        config,
        &json!({ "action": "fetch", "url": url, "purpose": purpose }),
    )?;
    Ok((
        format_resource_response(config, &response),
        response["status"] != "allowed",
    ))
}

pub(super) fn dependency(config: &ServerConfig, args: &Value) -> Result<(String, bool), String> {
    let ecosystem = required_text(args, "ecosystem", 32)?;
    let command = required_text(args, "command", 16_384)?;
    let reason = required_text(args, "reason", 4096)?;
    let kind = required_text(args, "kind", 64)?;
    let package = optional_text(args, "package", 256)?;
    let version = optional_text(args, "version", 256)?;
    let source = optional_text(args, "source", 4096)?;
    let need = json!({
        "ecosystem": ecosystem.to_ascii_lowercase(),
        "package": package,
        "version": version,
        "source": source,
        "command": command,
        "reason": reason,
        "kind": kind.to_ascii_lowercase(),
    });
    let preparation = request(
        config,
        &json!({
            "action": "dependency_request",
            "dependency": need,
            "purpose": reason,
        }),
    )?;
    let summary = format_resource_response(config, &preparation);
    if preparation["status"] != "prepared" {
        return Ok((summary, true));
    }
    let Some(request_id) = preparation["dependency_request"]["id"].as_str() else {
        return Ok((
            format!("{summary}\nThe broker did not return a dependency request ID."),
            true,
        ));
    };
    let retry = sandbox::run(config, command, 300, true, Some(ecosystem)).map_err(|error| {
        format!("Authorized offline dependency retry failed to start: {error:#}")
    })?;
    let result = request(
        config,
        &json!({
            "action": "dependency_retry_result",
            "dependency_request_id": request_id,
            "retry_succeeded": !retry.is_error,
            "purpose": "Record the result of the bounded offline dependency retry",
        }),
    );
    let report = result
        .ok()
        .map(|response| format_resource_response(config, &response))
        .unwrap_or_else(|| "The offline retry result could not be recorded.".into());
    Ok((
        format!("{summary}\n\n{}\n{report}", retry.text),
        retry.is_error,
    ))
}

fn request(config: &ServerConfig, request: &Value) -> Result<Value, String> {
    let socket = config
        .resource_socket
        .as_ref()
        .ok_or_else(|| "Kool.ad/e resource broker is unavailable".to_owned())?;
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("Cannot contact Kool.ad/e resource broker: {error}"))?;
    let encoded = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if encoded.len() > 16 * 1024 {
        return Err("Resource request exceeds the broker limit".into());
    }
    stream
        .write_all(&encoded)
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|error| format!("Cannot send request to Kool.ad/e resource broker: {error}"))?;
    let mut response = Vec::new();
    std::io::BufReader::new(stream)
        .take(1_000_001)
        .read_until(b'\n', &mut response)
        .map_err(|error| format!("Cannot read Kool.ad/e resource broker response: {error}"))?;
    if response.len() > 1_000_000 {
        return Err("Resource broker response exceeds the size limit".into());
    }
    serde_json::from_slice(&response)
        .map_err(|error| format!("Invalid resource broker response: {error}"))
}

fn format_resource_response(config: &ServerConfig, response: &Value) -> String {
    let mut parts = vec![
        response["summary"]
            .as_str()
            .unwrap_or("Resource request finished.")
            .to_owned(),
    ];
    if let Some(content) = response["content"].as_str() {
        parts.push(content.to_owned());
    }
    if let Some(path) = response["path"].as_str()
        && let Some(relative) = config
            .resource_cache
            .as_deref()
            .and_then(|cache| Path::new(path).strip_prefix(cache).ok())
    {
        parts.push(format!(
            "Saved file: /tmp/koolade-resource-files/{}",
            relative.display()
        ));
    }
    parts.join("\n")
}

fn required_text<'a>(args: &'a Value, field: &str, limit: usize) -> Result<&'a str, String> {
    args[field]
        .as_str()
        .filter(|value| !value.trim().is_empty() && value.len() <= limit)
        .ok_or_else(|| format!("A valid {field} value is required"))
}

fn optional_text<'a>(
    args: &'a Value,
    field: &str,
    limit: usize,
) -> Result<Option<&'a str>, String> {
    let value = args[field]
        .as_str()
        .filter(|value| !value.trim().is_empty());
    if value.is_some_and(|value| value.len() > limit) {
        return Err(format!("The {field} value is too long"));
    }
    Ok(value)
}
