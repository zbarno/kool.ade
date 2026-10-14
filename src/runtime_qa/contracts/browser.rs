use serde::{Deserialize, Serialize};
use url::{Host, Url};

use super::{is_sha256, is_uuid};

/// App-owned, already-running task service authorized for this QA run.
/// Its container is attached only to the private task network and publishes no
/// host ports; the Docker controller still verifies these facts with Engine.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaTaskServiceLeaseV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_uid: String,
    pub task_container_id: String,
    pub task_network_id: String,
    pub preview_port: u16,
    pub task_build_sha: String,
    pub approved_spec_hash: String,
    pub startup_recipe_ref: String,
    pub startup_recipe_sha256: String,
    pub internal_network_only: bool,
    pub published_ports: bool,
    pub expires_at_unix_ms: u64,
}

impl QaTaskServiceLeaseV1 {
    pub fn validate_for(&self, request: &super::QaRunRequestV1) -> Result<(), String> {
        if self.schema_version != 1
            || self.qa_run_id != request.qa_run_id
            || self.task_uid != request.parent_task_uid
            || !is_container_id(&self.task_container_id)
            || !is_container_id(&self.task_network_id)
            || !(1024..=u16::MAX).contains(&self.preview_port)
            || self.task_build_sha != request.build_artifact_sha256
            || self.approved_spec_hash != request.approved_spec_hash
            || self.startup_recipe_ref != request.startup_recipe_ref
            || self.startup_recipe_sha256 != request.startup_recipe_sha256
            || !is_sha256(&self.startup_recipe_sha256)
            || !self.internal_network_only
            || self.published_ports
            || self.expires_at_unix_ms <= now_unix_ms()
        {
            return Err(
                "QA task service lease is stale or outside the approved task and network scope"
                    .into(),
            );
        }
        Ok(())
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Records the app-owned path from a single task service through one two-network relay.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaPreviewRelayBindingV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_uid: String,
    pub task_execution_id: String,
    pub task_container_id: String,
    pub relay_container_id: String,
    pub browser_container_id: Option<String>,
    pub task_network_id: String,
    pub relay_browser_network_id: String,
    pub target_preview_port: u16,
    pub relay_port: u16,
    pub preview_origin: String,
    pub task_build_sha: String,
    pub approved_spec_hash: String,
    pub startup_recipe_id: String,
    pub startup_recipe_sha256: String,
    pub internal_networks_only: bool,
    pub published_ports: bool,
    pub recorded_at_unix_ms: u64,
}

impl QaPreviewRelayBindingV1 {
    /// Validate the task-service-to-relay path before an owned browser
    /// container exists. Its actual ID is added and persisted before start.
    pub fn validate_prelaunch(&self) -> Result<(), String> {
        self.validate_inner(false)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_inner(true)
    }

    fn validate_inner(&self, require_browser: bool) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.qa_run_id)
            || !is_uuid(&self.task_uid)
            || !is_uuid(&self.task_execution_id)
            || !is_container_id(&self.task_container_id)
            || !is_container_id(&self.relay_container_id)
            || self
                .browser_container_id
                .as_deref()
                .is_some_and(|id| !is_container_id(id))
            || (require_browser && self.browser_container_id.is_none())
            || self.task_container_id == self.relay_container_id
            || self
                .browser_container_id
                .as_deref()
                .is_some_and(|id| self.task_container_id == id || self.relay_container_id == id)
            || !is_container_id(&self.task_network_id)
            || !is_container_id(&self.relay_browser_network_id)
            || self.task_network_id == self.relay_browser_network_id
            || !(1024..=u16::MAX).contains(&self.target_preview_port)
            || self.relay_port != 8080
            || !is_sha256(&self.task_build_sha)
            || !is_sha256(&self.approved_spec_hash)
            || !valid_recipe_id(&self.startup_recipe_id)
            || !is_sha256(&self.startup_recipe_sha256)
            || !self.internal_networks_only
            || self.published_ports
            || self.recorded_at_unix_ms == 0
        {
            return Err("preview relay is not bound to one internal task and browser path".into());
        }
        let origin =
            Url::parse(&self.preview_origin).map_err(|_| "preview relay origin is invalid")?;
        if origin.scheme() != "http"
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !matches!(origin.host(), Some(Host::Domain(host)) if host == "qa-preview")
            || origin.port_or_known_default() != Some(self.relay_port)
        {
            return Err("preview relay origin must be its exact internal DNS name and port".into());
        }
        Ok(())
    }

    /// Validate every navigation, redirect, frame, and websocket against the sole relay origin.
    pub fn validate_request_url(&self, raw: &str) -> Result<(), String> {
        self.validate()?;
        let parsed = Url::parse(raw).map_err(|_| "browser request URL is invalid")?;
        if !matches!(parsed.scheme(), "http" | "ws")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || !matches!(parsed.host(), Some(Host::Domain(host)) if host == "qa-preview")
            || parsed.port_or_known_default() != Some(self.relay_port)
        {
            return Err("browser request escaped the exact task preview relay".into());
        }
        let mut normalized = parsed;
        if normalized.scheme() == "ws" {
            normalized
                .set_scheme("http")
                .map_err(|_| "browser websocket origin is invalid")?;
        }
        if normalized.origin()
            != Url::parse(&self.preview_origin)
                .map_err(|_| "relay origin is invalid")?
                .origin()
        {
            return Err("browser request escaped the exact task preview relay".into());
        }
        Ok(())
    }
}

pub fn pinned_browser_qa_image(
    architecture: &str,
) -> Result<crate::execution::docker::images::SelectedImage, String> {
    let lock = crate::execution::docker::images::ImageLock::bundled()
        .map_err(|failure| failure.to_string())?;
    lock.select(
        crate::execution::docker::images::RuntimeProfile::BrowserQa,
        architecture,
    )
    .map_err(|failure| failure.message)
}

fn is_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_recipe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
