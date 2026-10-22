use crate::{
    execution::{
        contract::ResourceBudgetV1,
        docker::{
            images::SelectedImage,
            qa_container::{
                QaArtifactMount, QaContainerUpload, QaDriverContainerSpec, QaDriverNetwork,
            },
        },
        session::ExecutionSession,
    },
    runtime_qa::{
        QaDriverOutputV1, QaPlatform, QaPreviewRelayBindingV1, QaRunRequestV1, QaScenarioDriver,
        QaScenarioV1, QaTargetAuthorization, QaTaskServiceLeaseV1, QaValidatedTargetV1,
        RuntimeQaStore,
    },
};

#[cfg(test)]
#[path = "docker/tests.rs"]
mod tests;

/// Executes one scenario in a disposable, image-lock selected Docker driver.
/// Artifact bytes and any browser service/relay binding must be resolved and
/// validated by the app before constructing this adapter.
pub(crate) struct DockerQaScenarioDriver<'a> {
    pub session: &'a ExecutionSession,
    pub request: &'a QaRunRequestV1,
    pub image: &'a SelectedImage,
    pub artifact_mount: QaArtifactMount,
    pub artifact_upload: &'a QaContainerUpload,
    pub dependency_caches: &'a [crate::harness::resource_bridge::CacheSnapshot],
    pub resources: &'a ResourceBudgetV1,
    pub desktop_target: Option<&'a QaValidatedTargetV1>,
    pub service_lease: Option<&'a QaTaskServiceLeaseV1>,
    pub relay_binding: Option<&'a QaPreviewRelayBindingV1>,
    pub store: &'a RuntimeQaStore,
    pub cancellation: &'a mut tokio::sync::watch::Receiver<bool>,
}

impl QaScenarioDriver for DockerQaScenarioDriver<'_> {
    fn platform(&self) -> QaPlatform {
        self.request
            .relevant_platforms
            .first()
            .copied()
            .unwrap_or(QaPlatform::Web)
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        scenario: &QaScenarioV1,
        authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        if request != self.request {
            return Err("Docker QA driver received a different immutable request".into());
        }
        let platform = self.platform();
        let (driver, network, lease, relay, request_bytes) = match (platform, authorization) {
            (QaPlatform::Web, QaTargetAuthorization::Web(lease)) => {
                if self.service_lease != Some(*lease) {
                    return Err(
                        "browser QA service lease differs from its app-owned binding".into(),
                    );
                }
                let relay = self
                    .relay_binding
                    .ok_or_else(|| "browser QA has no app-owned preview relay".to_owned())?;
                (
                    "browser",
                    QaDriverNetwork::InternalPreviewRelay {
                        network_id: relay.relay_browser_network_id.clone(),
                        relay_container_id: relay.relay_container_id.clone(),
                    },
                    Some(*lease),
                    Some(relay),
                    crate::runtime_qa::browser_driver::build_browser_request(request, scenario)?,
                )
            }
            (QaPlatform::LinuxDesktop, QaTargetAuthorization::LinuxDesktop(target)) => {
                if self.desktop_target != Some(*target) {
                    return Err("desktop QA target differs from its app-owned binding".into());
                }
                let envelope = serde_json::json!({
                    "schema_version": 1,
                    "request": request,
                    "scenario": scenario,
                    "target": target,
                });
                (
                    "desktop",
                    QaDriverNetwork::Disabled,
                    None,
                    None,
                    serde_json::to_vec(&envelope)
                        .map_err(|_| "desktop QA request could not be encoded")?,
                )
            }
            _ => return Err("QA authorization does not match the selected platform driver".into()),
        };
        let request_upload = QaContainerUpload::request_file(&request_bytes, 256 * 1024)
            .map_err(|error| error.message)?;
        let mut uploads = vec![self.artifact_upload.clone(), request_upload];
        let driver_caches = if driver == "desktop" {
            self.dependency_caches
        } else {
            &[]
        };
        for cache in driver_caches {
            let target = match cache.cache_ref.ecosystem {
                crate::execution::contract::DependencyCacheEcosystemV1::Npm => {
                    "/home/executor/.npm"
                }
                crate::execution::contract::DependencyCacheEcosystemV1::Cargo => {
                    "/home/executor/.cargo"
                }
            };
            uploads.push(QaContainerUpload {
                target: target.into(),
                archive: crate::execution::docker::qa_container::archive_for_runtime_user(
                    cache,
                    &self.image.runtime_user,
                )
                .map_err(|error| error.message)?,
            });
        }
        let artifacts = [self.artifact_mount.clone()];
        let mut relay = relay.cloned();
        let container_id = {
            let spec = QaDriverContainerSpec {
                owner_id: &request.execution_session_id,
                request,
                task_uid: &request.parent_task_uid,
                driver,
                image: self.image,
                artifacts: &artifacts,
                dependency_caches: driver_caches,
                network,
                service_lease: lease,
                relay_binding: relay.as_ref(),
                resources: self.resources,
            };
            self.session
                .create_qa_driver(&spec, &uploads, self.cancellation)
                .map_err(|error| error.message)?
        };
        if let Some(binding) = relay.as_mut() {
            binding.browser_container_id = Some(container_id.clone());
            if let Err(error) = self
                .store
                .save_relay_binding(request, &scenario.id, binding)
            {
                let _ = self.session.cleanup_qa_driver(&container_id);
                return Err(format!(
                    "QA driver identity could not be durably bound: {error}"
                ));
            }
        }
        let run_result = self.session.run_qa_driver(
            &container_id,
            &QaDriverContainerSpec {
                owner_id: &request.execution_session_id,
                request,
                task_uid: &request.parent_task_uid,
                driver,
                image: self.image,
                artifacts: &artifacts,
                dependency_caches: driver_caches,
                network: match &relay {
                    Some(binding) => QaDriverNetwork::InternalPreviewRelay {
                        network_id: binding.relay_browser_network_id.clone(),
                        relay_container_id: binding.relay_container_id.clone(),
                    },
                    None => QaDriverNetwork::Disabled,
                },
                service_lease: lease,
                relay_binding: relay.as_ref(),
                resources: self.resources,
            },
            self.cancellation,
        );
        let cleanup_result = self.session.cleanup_qa_driver(&container_id);
        let output = match (run_result, cleanup_result) {
            (Ok(output), Ok(())) => output,
            (Err(error), _) => return Err(error.message),
            (Ok(_), Err(error)) => return Err(error.message),
        };
        match platform {
            QaPlatform::Web => {
                let mut report = crate::runtime_qa::browser_driver::parse_browser_output(
                    &output.stdout,
                    request,
                    scenario,
                )?;
                apply_browser_exit_status(output.exit_code, &mut report);
                Ok(report)
            }
            QaPlatform::LinuxDesktop => {
                if output.exit_code != 0 {
                    return Err(format!(
                        "isolated {driver} QA driver exited with status {}",
                        output.exit_code
                    ));
                }
                serde_json::from_slice(&output.stdout)
                    .map_err(|_| "isolated desktop QA driver returned malformed evidence".into())
            }
        }
    }
}

fn apply_browser_exit_status(exit_code: i64, report: &mut QaDriverOutputV1) {
    if exit_code == 0
        || report.failure.is_some()
        || !report.completed
        || report
            .assertions
            .iter()
            .any(|assertion| assertion.status != crate::runtime_qa::QaAssertionStatus::Passed)
    {
        return;
    }
    report.completed = false;
    report.failure = Some(format!(
        "browser QA driver exited with status {exit_code} without reporting a failed or incomplete assertion"
    ));
}
