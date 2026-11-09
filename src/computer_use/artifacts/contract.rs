use serde::{Deserialize, Serialize};

use crate::computer_use::ComputerIdentityV1;

pub const COMPUTER_ARTIFACT_MAX_BYTES_V1: u64 = 64 * 1024 * 1024;
pub const COMPUTER_ARTIFACT_STORE_QUOTA_BYTES_V1: u64 = 1024 * 1024 * 1024;
pub const COMPUTER_CLIPBOARD_MAX_BYTES_V1: usize = 64 * 1024;
const SCHEMA_VERSION: u16 = 1;

/// The transfer's source and destination are part of its authority identity.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerArtifactFlowV1 {
    TaskArtifactToGuest,
    GuestArtifactToTask,
    UserApprovedFileToPersonalGuest,
    UserDownloadFromGuest,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerClipboardDirectionV1 {
    CopyToGuest,
    PasteFromGuest,
}

/// Immutable controller-issued authorization for one named file and flow.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerTransferGrantV1 {
    pub schema_version: u16,
    pub grant_id: String,
    pub identity: ComputerIdentityV1,
    pub flow: ComputerArtifactFlowV1,
    pub task_revision: String,
    pub relative_path: String,
    pub max_bytes: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub user_approved: bool,
    /// Canonical hash of a persisted human-approved QA build record. Present
    /// only for the dedicated task-build-to-guest permission path.
    #[serde(default)]
    pub approval_receipt_sha256: Option<String>,
}

/// Inputs are accepted only from trusted app/controller code after its user or
/// task authorization path has completed. Provider arguments cannot create a grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerTransferGrantRequestV1 {
    pub identity: ComputerIdentityV1,
    pub flow: ComputerArtifactFlowV1,
    pub task_revision: String,
    pub relative_path: String,
    pub max_bytes: u64,
    pub now_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub user_approved: bool,
}

/// Audit metadata for a staged private artifact. It never contains artifact bytes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerArtifactManifestV1 {
    pub schema_version: u16,
    pub artifact_id: String,
    pub grant_id: String,
    pub identity: ComputerIdentityV1,
    pub flow: ComputerArtifactFlowV1,
    pub task_revision: String,
    pub relative_path: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub media_type: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lease_generation: u64,
    pub approval_generation: u64,
    #[serde(default)]
    pub approval_receipt_sha256: Option<String>,
    /// Payload text is not included in task/model activity by this contract.
    pub private_content: bool,
}

impl ComputerArtifactManifestV1 {
    pub(crate) fn new(
        grant: &ComputerTransferGrantV1,
        artifact_id: String,
        display_name: String,
        size_bytes: u64,
        sha256: String,
        media_type: String,
        now_unix_ms: u64,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            artifact_id,
            grant_id: grant.grant_id.clone(),
            identity: grant.identity.clone(),
            flow: grant.flow,
            task_revision: grant.task_revision.clone(),
            relative_path: grant.relative_path.clone(),
            display_name,
            size_bytes,
            sha256,
            media_type,
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: grant.expires_at_unix_ms,
            lease_generation: grant.lease_generation,
            approval_generation: grant.approval_generation,
            approval_receipt_sha256: grant.approval_receipt_sha256.clone(),
            private_content: true,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != SCHEMA_VERSION
            || !canonical_uuid(&self.artifact_id)
            || !canonical_uuid(&self.grant_id)
            || self.identity.validate().is_err()
            || !valid_revision(&self.task_revision)
            || !super::validation::valid_relative_path(&self.relative_path)
            || !super::validation::valid_display_name(&self.display_name)
            || self.size_bytes == 0
            || self.size_bytes > COMPUTER_ARTIFACT_MAX_BYTES_V1
            || !valid_sha256(&self.sha256)
            || !valid_manifest_media_type(self)
            || self.created_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.created_at_unix_ms
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self
                .approval_receipt_sha256
                .as_deref()
                .is_some_and(|digest| !valid_bare_sha256(digest))
            || (self.flow != ComputerArtifactFlowV1::TaskArtifactToGuest
                && self.approval_receipt_sha256.is_some())
            || !self.private_content
        {
            return Err("Computer Use artifact manifest is invalid");
        }
        Ok(())
    }
}

fn valid_manifest_media_type(manifest: &ComputerArtifactManifestV1) -> bool {
    super::validation::media_type_for_name(&manifest.display_name).as_deref()
        == Some(manifest.media_type.as_str())
        || (manifest.flow == ComputerArtifactFlowV1::TaskArtifactToGuest
            && super::validation::media_type_for_name(&manifest.display_name).is_none()
            && manifest.media_type == "application/octet-stream")
}

fn valid_bare_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Small result returned to UI/task activity; bytes are fetched separately.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerArtifactReceiptV1 {
    pub artifact_id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub media_type: String,
    pub expires_at_unix_ms: u64,
}

/// A single, expiring clipboard action. Clipboard bytes are never persisted.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerClipboardGrantV1 {
    pub schema_version: u16,
    pub grant_id: String,
    pub identity: ComputerIdentityV1,
    pub direction: ComputerClipboardDirectionV1,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub user_approved: bool,
    pub visible_indicator: bool,
}

impl ComputerTransferGrantV1 {
    pub(crate) fn from_approved_task_app_request(
        request: ComputerTransferGrantRequestV1,
        approval_receipt_sha256: String,
    ) -> Result<Self, &'static str> {
        if request.flow != ComputerArtifactFlowV1::TaskArtifactToGuest
            || request.user_approved
            || !valid_bare_sha256(&approval_receipt_sha256)
        {
            return Err("task app transfer requires a validated persisted approval receipt");
        }
        let mut grant = Self::from_request(request)?;
        grant.user_approved = true;
        grant.approval_receipt_sha256 = Some(approval_receipt_sha256);
        Ok(grant)
    }

    pub(crate) fn from_request(
        request: ComputerTransferGrantRequestV1,
    ) -> Result<Self, &'static str> {
        request.identity.validate()?;
        if !valid_revision(&request.task_revision)
            || !super::validation::valid_relative_path(&request.relative_path)
            || request.max_bytes == 0
            || request.max_bytes > COMPUTER_ARTIFACT_MAX_BYTES_V1
            || request.now_unix_ms == 0
            || request.expires_at_unix_ms <= request.now_unix_ms
            || request.expires_at_unix_ms - request.now_unix_ms > 15 * 60 * 1000
            || request.lease_generation == 0
            || request.approval_generation == 0
        {
            return Err("Computer Use transfer grant is invalid or exceeds its bounds");
        }
        match request.flow {
            ComputerArtifactFlowV1::UserApprovedFileToPersonalGuest if !request.user_approved => {
                return Err("personal guest file transfer requires explicit user approval");
            }
            ComputerArtifactFlowV1::TaskArtifactToGuest
            | ComputerArtifactFlowV1::GuestArtifactToTask
                if request.user_approved =>
            {
                return Err("task artifact grants cannot claim user file approval");
            }
            ComputerArtifactFlowV1::UserDownloadFromGuest if !request.user_approved => {
                return Err("guest download requires explicit user approval");
            }
            _ => {}
        }
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            grant_id: uuid::Uuid::new_v4().to_string(),
            identity: request.identity,
            flow: request.flow,
            task_revision: request.task_revision,
            relative_path: request.relative_path,
            max_bytes: request.max_bytes,
            issued_at_unix_ms: request.now_unix_ms,
            expires_at_unix_ms: request.expires_at_unix_ms,
            lease_generation: request.lease_generation,
            approval_generation: request.approval_generation,
            user_approved: request.user_approved,
            approval_receipt_sha256: None,
        })
    }

    pub(crate) fn validate(&self, now_unix_ms: u64) -> Result<(), &'static str> {
        if self.schema_version != SCHEMA_VERSION
            || !canonical_uuid(&self.grant_id)
            || self.identity.validate().is_err()
            || !valid_revision(&self.task_revision)
            || !super::validation::valid_relative_path(&self.relative_path)
            || self.max_bytes == 0
            || self.max_bytes > COMPUTER_ARTIFACT_MAX_BYTES_V1
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= now_unix_ms
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.expires_at_unix_ms - self.issued_at_unix_ms > 15 * 60 * 1000
            || self.lease_generation == 0
            || self.approval_generation == 0
            || self
                .approval_receipt_sha256
                .as_deref()
                .is_some_and(|digest| !valid_bare_sha256(digest))
            || (self.flow == ComputerArtifactFlowV1::TaskArtifactToGuest
                && self.user_approved != self.approval_receipt_sha256.is_some())
            || (self.flow != ComputerArtifactFlowV1::TaskArtifactToGuest
                && self.approval_receipt_sha256.is_some())
        {
            return Err("Computer Use transfer grant is expired, invalid, or exceeds its bounds");
        }
        match self.flow {
            ComputerArtifactFlowV1::UserApprovedFileToPersonalGuest if !self.user_approved => {
                return Err("personal guest file transfer requires explicit user approval");
            }
            ComputerArtifactFlowV1::GuestArtifactToTask if self.user_approved => {
                return Err("task artifact grants cannot claim user file approval");
            }
            ComputerArtifactFlowV1::UserDownloadFromGuest if !self.user_approved => {
                return Err("guest download requires explicit user approval");
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn new_clipboard_grant(
    identity: ComputerIdentityV1,
    direction: ComputerClipboardDirectionV1,
    now_unix_ms: u64,
    expires_at_unix_ms: u64,
    lease_generation: u64,
    approval_generation: u64,
    user_approved: bool,
) -> Result<ComputerClipboardGrantV1, &'static str> {
    identity.validate()?;
    if !user_approved
        || now_unix_ms == 0
        || expires_at_unix_ms <= now_unix_ms
        || expires_at_unix_ms - now_unix_ms > 5 * 60 * 1000
        || lease_generation == 0
        || approval_generation == 0
    {
        return Err("clipboard access requires a visible, explicit, short-lived user grant");
    }
    Ok(ComputerClipboardGrantV1 {
        schema_version: SCHEMA_VERSION,
        grant_id: uuid::Uuid::new_v4().to_string(),
        identity,
        direction,
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms,
        lease_generation,
        approval_generation,
        user_approved,
        visible_indicator: true,
    })
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn valid_revision(value: &str) -> bool {
    (40..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
