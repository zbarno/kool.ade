use super::super::ResourceRequest;
use super::ResourceBridge;
use crate::harness::{
    DependencyKind, DependencyNeed, DependencyRequest, DependencyRequestStatus, PackageEcosystem,
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

const MAX_RESPONSE_BYTES: usize = 256 * 1024;

impl ResourceBridge {
    pub(crate) fn request_system_tools(
        &self,
        need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        self.request_system_tools_inner(need)
    }

    #[cfg(test)]
    pub(crate) fn request_locked_dependency(
        &self,
        need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        if !matches!(
            need.ecosystem,
            PackageEcosystem::Npm
                | PackageEcosystem::Cargo
                | PackageEcosystem::Pip
                | PackageEcosystem::Go
        ) || need.kind != DependencyKind::ExistingRestore
        {
            return Err(
                "runtime package dependency request must be a locked npm, Cargo, pip, or Go restore"
                    .into(),
            );
        }
        let dependency = self.request_dependency_inner(need)?;
        self.stage_locked_dependency_cache(&dependency)?;
        Ok(dependency)
    }

    #[cfg(test)]
    pub(crate) fn request_locked_dependency_for_fixture(
        &self,
        need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        if !matches!(
            need.ecosystem,
            PackageEcosystem::Npm
                | PackageEcosystem::Cargo
                | PackageEcosystem::Pip
                | PackageEcosystem::Go
        ) || need.kind != DependencyKind::ExistingRestore
        {
            return Err(
                "runtime package dependency request must be a locked npm, Cargo, pip, or Go restore"
                    .into(),
            );
        }
        self.request_dependency_inner(need)
    }

    pub(super) fn request_system_tools_inner(
        &self,
        need: DependencyNeed,
    ) -> Result<DependencyRequest, String> {
        if need.ecosystem != PackageEcosystem::System || need.kind != DependencyKind::SystemTool {
            return Err("runtime prerequisite request is not a system tool".into());
        }
        self.request_dependency_inner(need)
    }

    fn request_dependency_inner(&self, need: DependencyNeed) -> Result<DependencyRequest, String> {
        let Some(task_id) = self.task_id.as_deref() else {
            return Err("resource broker is not bound to a task".into());
        };
        if self.cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("runtime recipe was cancelled before prerequisite review".into());
        }

        let request = ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(need.clone()),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: need.reason.clone(),
        };
        let mut stream = UnixStream::connect(&self.socket)
            .map_err(|error| format!("could not connect to the task dependency broker: {error}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| format!("could not bound dependency request writes: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|error| format!("could not poll dependency authorization: {error}"))?;
        serde_json::to_writer(
            &mut stream,
            &super::super::BrokerWireRequest::Resource(request),
        )
        .map_err(|error| format!("could not encode dependency request: {error}"))?;
        stream
            .write_all(b"\n")
            .and_then(|()| stream.flush())
            .map_err(|error| format!("could not submit dependency request: {error}"))?;

        let mut response_bytes = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            if self.cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err("runtime recipe was cancelled during prerequisite review".into());
            }
            match stream.read(&mut buffer) {
                Ok(0) => {
                    return Err("dependency broker closed before returning its decision".into());
                }
                Ok(read) => {
                    response_bytes.extend_from_slice(&buffer[..read]);
                    if response_bytes.len() > MAX_RESPONSE_BYTES {
                        return Err("dependency broker response exceeded its size limit".into());
                    }
                    if response_bytes.contains(&b'\n') {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => {
                    return Err(format!("dependency broker response failed: {error}"));
                }
            }
        }
        let line = response_bytes
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        let response: super::super::ResourceResponse = serde_json::from_slice(line)
            .map_err(|error| format!("dependency broker returned invalid evidence: {error}"))?;
        let dependency = response
            .dependency_request
            .ok_or_else(|| response.summary.clone())?;
        let evidence_matches = dependency.task_id == task_id
            && dependency.need.ecosystem == need.ecosystem
            && dependency.need.kind == need.kind
            && dependency.need.package == need.package
            && dependency.need.version == need.version
            && dependency.need.source == need.source
            && dependency.need.command == need.command
            && dependency.need.reason == need.reason
            && match need.ecosystem {
                PackageEcosystem::System => !dependency.need.introduced_packages.is_empty(),
                PackageEcosystem::Npm
                | PackageEcosystem::Cargo
                | PackageEcosystem::Pip
                | PackageEcosystem::Go => dependency.need.lockfile_identity.is_some(),
                _ => false,
            }
            && dependency.status == DependencyRequestStatus::Prepared
            && response.status == "prepared";
        if !evidence_matches {
            return Err(response.summary);
        }
        Ok(dependency)
    }

    #[cfg(test)]
    fn stage_locked_dependency_cache(&self, dependency: &DependencyRequest) -> Result<(), String> {
        let Some(task_id) = self.task_id.as_deref() else {
            return Err("resource broker is not bound to a task".into());
        };
        let session = self
            .execution_session
            .lock()
            .map_err(|_| "recipe execution session binding is unavailable".to_owned())?
            .clone()
            .ok_or("locked dependency restore has no bound recipe execution session")?;
        let ecosystem = match dependency.need.ecosystem {
            PackageEcosystem::Npm => crate::execution::contract::DependencyCacheEcosystemV1::Npm,
            PackageEcosystem::Cargo => {
                crate::execution::contract::DependencyCacheEcosystemV1::Cargo
            }
            PackageEcosystem::Pip => crate::execution::contract::DependencyCacheEcosystemV1::Pip,
            PackageEcosystem::Go => crate::execution::contract::DependencyCacheEcosystemV1::Go,
            _ => return Err("prepared dependency has no verified cache adapter".into()),
        };
        let resolved_integrity = dependency
            .need
            .lockfile_identity
            .as_deref()
            .ok_or("prepared locked dependency has no lockfile identity")?;
        let target_arch = session
            .cache_target_arch()
            .ok_or("recipe execution session architecture is unavailable")?;
        let snapshot = self
            .cache_snapshot(
                task_id,
                ecosystem,
                &session.workspace().project_id,
                resolved_integrity,
                &target_arch,
                session.target_id(),
            )
            .map_err(|error| format!("authorized package cache snapshot failed: {error:#}"))?;
        session
            .stage_dependency_cache_for_request(&dependency.id, snapshot)
            .map_err(|error| {
                format!(
                    "authorized package cache could not be staged: {}",
                    error.message
                )
            })
    }
}
