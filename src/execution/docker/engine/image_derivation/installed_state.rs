use super::super::bollard_api::BollardApi;
use super::super::{EngineFuture, PrerequisiteCode, PrerequisiteFailure, failure};
use bollard::query_parameters::{
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, WaitContainerOptionsBuilder,
};
use futures_util::StreamExt;
use std::collections::BTreeMap;

pub(super) fn inspect<'a>(
    api: &'a BollardApi,
    image_id: &'a str,
) -> EngineFuture<
    'a,
    Result<
        BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        PrerequisiteFailure,
    >,
> {
    Box::pin(async move {
        let config = bollard::models::ContainerCreateBody {
            image: Some(image_id.to_owned()),
            entrypoint: Some(vec!["/usr/bin/dpkg-query".into()]),
            cmd: Some(vec![
                "-W".into(),
                "-f=${Package}=${Version}=${Architecture}=${Multi-Arch}=${db:Status-Status}\\n"
                    .into(),
            ]),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            network_disabled: Some(true),
            host_config: Some(bollard::models::HostConfig {
                network_mode: Some("none".into()),
                readonly_rootfs: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = api
            .docker
            .create_container(None, config)
            .await
            .map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Docker could not inspect the base image package database offline",
                    false,
                )
            })?;
        let result = run_query(&api.docker, &created.id).await;
        let _ = api
            .docker
            .remove_container(
                &created.id,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
        result
    })
}

async fn run_query(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>, PrerequisiteFailure>
{
    docker
        .start_container(container_id, None)
        .await
        .map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker could not start the offline base package query",
                false,
            )
        })?;
    let mut wait = docker.wait_container(
        container_id,
        Some(
            WaitContainerOptionsBuilder::default()
                .condition("not-running")
                .build(),
        ),
    );
    let mut exit_code = None;
    while let Some(result) = wait.next().await {
        exit_code = Some(
            result
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Base package query did not finish",
                        false,
                    )
                })?
                .status_code,
        );
    }
    if exit_code != Some(0) {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Base package query failed",
            false,
        ));
    }
    let mut logs = docker.logs(
        container_id,
        Some(LogsOptionsBuilder::default().stdout(true).build()),
    );
    let mut output = String::new();
    while let Some(log) = logs.next().await {
        output.push_str(&String::from_utf8_lossy(
            &log.map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Base package query output could not be read",
                    false,
                )
            })?
            .into_bytes(),
        ));
        if output.len() > 8 * 1024 * 1024 {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Base package state exceeded its limit",
                false,
            ));
        }
    }
    let mut packages = BTreeMap::new();
    for line in output.lines() {
        let mut fields = line.trim().split('=');
        let (name, version, architecture, multi_arch, status) = (
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
        );
        if status != "installed" {
            continue;
        }
        if fields.next().is_some()
            || !valid_name(name)
            || version.is_empty()
            || !(architecture == "all" || architecture == "amd64" || architecture == "arm64")
            || !(multi_arch.is_empty()
                || matches!(multi_arch, "no" | "same" | "foreign" | "allowed"))
            || packages
                .insert(
                    name.to_owned(),
                    crate::harness::resource_bridge::InstalledPackage {
                        version: version.to_owned(),
                        architecture: architecture.to_owned(),
                        multi_arch: (!multi_arch.is_empty() && multi_arch != "no")
                            .then(|| multi_arch.to_owned()),
                    },
                )
                .is_some()
        {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Base package state was malformed or ambiguous",
                false,
            ));
        }
        if packages.len() > 100_000 {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Base package count exceeded its limit",
                false,
            ));
        }
    }
    Ok(packages)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+.-".contains(&byte)
        })
}
