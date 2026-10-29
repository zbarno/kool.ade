mod disconnect;

use super::super::{
    BrokerWireRequest, CacheSnapshotRequest, CacheSnapshotResponse, MAX_REQUEST_BYTES,
    ResourceResponse, write_json,
};
use super::BrokerContext;
use super::dependency_flow::{activity::store_dependency_request, prepare_request};
use std::{
    io::{self, BufRead, Read, Write},
    os::unix::net::UnixStream,
};

pub(super) fn serve(mut client: UnixStream, context: &BrokerContext<'_>) -> io::Result<()> {
    client.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    client.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut request = Vec::new();
    let read = std::io::BufReader::new(client.try_clone()?)
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_until(b'\n', &mut request)?;
    if read == 0 || request.len() > MAX_REQUEST_BYTES {
        return write_json(
            &mut client,
            &ResourceResponse {
                status: "rejected".into(),
                summary: "Resource request is empty or too large.".into(),
                content: None,
                path: None,
                dependency_request: None,
                dependency_result: None,
                preparation: None,
                bytes: 0,
            },
        );
    }
    let parsed = serde_json::from_slice::<BrokerWireRequest>(
        request.strip_suffix(b"\n").unwrap_or(&request),
    );
    let monitor = disconnect::ClientDisconnectMonitor::start(&client)?;
    let response = match parsed {
        Ok(BrokerWireRequest::CacheSnapshot(request)) => {
            let snapshot = prepare_cache_snapshot(context, &request).map_err(io::Error::other)?;
            let header = CacheSnapshotResponse {
                status: "snapshot".into(),
                cache_ref: snapshot.cache_ref.clone(),
                manifest: snapshot.manifest.clone(),
                archive_bytes: snapshot.archive.len(),
            };
            serde_json::to_writer(&mut client, &header)?;
            client.write_all(b"\n")?;
            client.write_all(&snapshot.archive)?;
            return client.flush();
        }
        Ok(BrokerWireRequest::Resource(request)) => {
            let started = std::time::Instant::now();
            let mut response = match prepare_request(context, &request, monitor.disconnected()) {
                Ok(response) => response,
                Err(error) => ResourceResponse {
                    status: "error".into(),
                    summary: format!("Resource request could not be completed: {error:#}"),
                    content: None,
                    path: None,
                    dependency_request: None,
                    dependency_result: None,
                    preparation: None,
                    bytes: 0,
                },
            };
            if let Some(dependency) = response.dependency_request.as_mut() {
                let preparation = dependency
                    .preparation
                    .get_or_insert_with(crate::harness::DependencyPreparationTelemetry::default);
                preparation.broker_duration_millis =
                    Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                response.preparation = Some(preparation.clone());
                response.dependency_result = response.preparation.clone();
            }
            response
        }
        Err(error) => ResourceResponse {
            status: "rejected".into(),
            summary: format!("Resource request was invalid: {error}"),
            content: None,
            path: None,
            dependency_request: None,
            dependency_result: None,
            preparation: None,
            bytes: 0,
        },
    };
    if let Some(request) = response.dependency_request.as_ref() {
        store_dependency_request(context, request);
        if request
            .preparation
            .as_ref()
            .and_then(|preparation| preparation.broker_duration_millis)
            .is_some()
        {
            let _ = context.progress.send(crate::harness::LiveProgress {
                activity: Some("Dependency preparation finished".into()),
                dependency_requests: vec![request.clone()],
                ..Default::default()
            });
        }
    }
    if response.status != "allowed"
        && response.status != "prepared"
        && let Ok(mut pending) = context.attention.lock()
        && pending.is_none()
    {
        *pending = Some(response.summary.clone());
    }
    drop(monitor);
    write_json(&mut client, &response)
}

fn prepare_cache_snapshot(
    context: &BrokerContext<'_>,
    request: &CacheSnapshotRequest,
) -> anyhow::Result<super::super::cache_snapshot::CacheSnapshot> {
    anyhow::ensure!(
        request.action == "cache_snapshot",
        "cache snapshot action is invalid"
    );
    let task_id = context
        .task_id
        .ok_or_else(|| anyhow::anyhow!("cache broker is not bound to a task"))?;
    let approved = {
        let requests = context
            .dependency
            .lock()
            .map_err(|_| anyhow::anyhow!("dependency authorization state is unavailable"))?;
        prepared_dependency(&requests, &request.dependency_request_id, task_id)?.clone()
    };
    anyhow::ensure!(
        approved.id == request.dependency_request_id
            && approved.task_id == task_id
            && approved.status == crate::harness::DependencyRequestStatus::Prepared,
        "cache snapshot does not match the prepared dependency authorization"
    );
    let ecosystem = match approved.need.ecosystem {
        crate::harness::PackageEcosystem::Npm => {
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        }
        crate::harness::PackageEcosystem::Cargo => {
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo
        }
        crate::harness::PackageEcosystem::Pip => {
            crate::execution::contract::DependencyCacheEcosystemV1::Pip
        }
        crate::harness::PackageEcosystem::Go => {
            crate::execution::contract::DependencyCacheEcosystemV1::Go
        }
        _ => anyhow::bail!("dependency ecosystem has no verified cache adapter"),
    };
    anyhow::ensure!(
        ecosystem == request.ecosystem,
        "cache ecosystem differs from authorization"
    );
    let resolved_integrity = approved.need.lockfile_identity.clone().unwrap_or_else(|| {
        use sha2::{Digest, Sha256};
        format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&approved.need).unwrap_or_default())
        )
    });
    let root = match ecosystem {
        crate::execution::contract::DependencyCacheEcosystemV1::Npm => {
            context.npm_cache.to_path_buf()
        }
        crate::execution::contract::DependencyCacheEcosystemV1::Cargo => {
            context.cargo_cache.to_path_buf()
        }
        crate::execution::contract::DependencyCacheEcosystemV1::Pip => {
            crate::harness::resource_bridge::pip::snapshot_root(
                context.pip_cache,
                &resolved_integrity,
            )?
        }
        crate::execution::contract::DependencyCacheEcosystemV1::Go => {
            crate::harness::resource_bridge::go::snapshot_root(
                context.go_cache,
                &resolved_integrity,
            )?
        }
    };
    let snapshot = super::super::cache_snapshot::CacheSnapshot::capture(
        &root,
        task_id,
        ecosystem,
        &request.project_id,
        &resolved_integrity,
        &request.target_arch,
        &request.target_id,
    )?;
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)?;
    Ok(snapshot)
}

fn prepared_dependency<'a>(
    requests: &'a [crate::harness::DependencyRequest],
    dependency_request_id: &str,
    task_id: &str,
) -> anyhow::Result<&'a crate::harness::DependencyRequest> {
    let request = requests
        .iter()
        .find(|request| request.id == dependency_request_id)
        .ok_or_else(|| {
            anyhow::anyhow!("no matching dependency request is available for cache transfer")
        })?;
    anyhow::ensure!(
        request.task_id == task_id
            && request.status == crate::harness::DependencyRequestStatus::Prepared,
        "cache snapshot does not match the prepared dependency authorization"
    );
    Ok(request)
}

#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;
