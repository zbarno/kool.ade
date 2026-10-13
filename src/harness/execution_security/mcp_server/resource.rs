use super::ServerConfig;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
};

use crate::harness::resource_bridge::{CacheSnapshot, CacheSnapshotRequest, CacheSnapshotResponse};

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

pub(super) fn dependency(
    config_path: &Path,
    config: &ServerConfig,
    args: &Value,
) -> Result<(String, bool), String> {
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
    let need: crate::harness::DependencyNeed = serde_json::from_value(need.clone())
        .map_err(|_| "Dependency request did not match the broker contract".to_owned())?;
    let binding = super::super::session_bridge::SessionBinding {
        session_id: config.session_id.clone(),
        capability_id: config.capability_id.clone(),
        task_id: config.task_id.clone(),
        workspace_id: config.workspace_id.clone(),
        access: "implementation".into(),
        target_id: config
            .target_id
            .clone()
            .ok_or_else(|| "cache target identity is unavailable".to_owned())?,
        target_arch: config.target_arch.clone(),
    };
    let remote_preparation = if binding.target_id != "local" {
        let emitted =
            super::super::session_bridge::request_dependency(config_path, &binding, need.clone())
                .map_err(|error| {
                format!("Authenticated worker dependency request failed: {error:#}")
            })?;
        if emitted.request.need != need {
            return Err("Remote worker changed the dependency request during relay".into());
        }
        Some(emitted)
    } else {
        None
    };
    let preparation_result = match remote_preparation.as_ref() {
        Some(remote) => Ok(remote.broker_response.clone()),
        None => request(
            config,
            &json!({
                "action": "dependency_request",
                "dependency": need,
                "purpose": reason,
            }),
        ),
    };
    let preparation = preparation_result?;
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
    let ecosystem = match ecosystem.to_ascii_lowercase().as_str() {
        "npm" => crate::execution::contract::DependencyCacheEcosystemV1::Npm,
        "cargo" => crate::execution::contract::DependencyCacheEcosystemV1::Cargo,
        _ => return Ok((summary, true)),
    };
    let project_id = config
        .workspace_id
        .split(':')
        .next()
        .ok_or_else(|| "MCP workspace project identity is missing".to_owned())?;
    let cache_snapshot = fetch_cache_snapshot(config, request_id, ecosystem, project_id)?;
    super::super::session_bridge::stage_cache(config_path, &binding, cache_snapshot)
        .map_err(|error| format!("Authorized package cache could not be staged: {error:#}"))?;
    let retry = super::super::session_bridge::call(config_path, &binding, command, 300)
        .map_err(|error| format!("Authorized offline Docker retry failed: {error:#}"))?;
    let retry_text = [retry.stdout, retry.stderr]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
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
        format!("{summary}\n\n{retry_text}\n{report}"),
        retry.is_error,
    ))
}

fn fetch_cache_snapshot(
    config: &ServerConfig,
    dependency_request_id: &str,
    ecosystem: crate::execution::contract::DependencyCacheEcosystemV1,
    project_id: &str,
) -> Result<CacheSnapshot, String> {
    let socket = config
        .resource_socket
        .as_ref()
        .ok_or_else(|| "Kool.ad/e resource broker is unavailable".to_owned())?;
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("Cannot contact Kool.ad/e cache broker: {error}"))?;
    let request = CacheSnapshotRequest {
        action: "cache_snapshot".into(),
        dependency_request_id: dependency_request_id.into(),
        ecosystem,
        project_id: project_id.into(),
        target_arch: config
            .target_arch
            .clone()
            .ok_or_else(|| "cache target architecture is unavailable".to_owned())?,
        target_id: config
            .target_id
            .clone()
            .ok_or_else(|| "cache target identity is unavailable".to_owned())?,
    };
    serde_json::to_writer(&mut stream, &request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    let mut reader = std::io::BufReader::new(stream);
    let mut header = Vec::new();
    (&mut reader)
        .take(4 * 1024 * 1024)
        .read_until(b'\n', &mut header)
        .map_err(|error| error.to_string())?;
    if header.len() > 4 * 1024 * 1024 {
        return Err("Cache snapshot manifest exceeds the limit".into());
    }
    let response: CacheSnapshotResponse = serde_json::from_slice(&header)
        .map_err(|error| format!("Invalid cache snapshot manifest: {error}"))?;
    if response.status != "snapshot"
        || response.archive_bytes as u64 > crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES
        || response.cache_ref.project_id != project_id
        || response.cache_ref.task_id != config.task_id
        || Some(response.cache_ref.target_id.as_str()) != config.target_id.as_deref()
    {
        return Err("Cache snapshot scope or size is invalid".into());
    }
    let mut archive = vec![0; response.archive_bytes];
    reader
        .read_exact(&mut archive)
        .map_err(|error| format!("Cache snapshot transfer was incomplete: {error}"))?;
    response
        .manifest
        .validate_ref(&response.cache_ref, &archive)
        .map_err(|_| "Cache snapshot integrity verification failed".to_owned())?;
    Ok(CacheSnapshot {
        cache_ref: response.cache_ref,
        manifest: response.manifest,
        archive,
    })
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
