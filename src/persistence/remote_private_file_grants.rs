//! User-local receipts authorizing exact private files for one remote execution.
//! The store contains metadata only; file bytes never enter this persistence path.

use crate::execution::remote::private_files::{
    RemotePrivateFileGrantV1, UserConfirmedPrivateFileGrantV1,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: u8 = 1;
const MAX_STORE_BYTES: u64 = 1024 * 1024;
const MAX_GRANTS: usize = 512;

mod store;
#[cfg(test)]
#[path = "remote_private_file_grants/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrivateFileGrantRevocationV1 {
    pub grant_id: String,
    pub revoked_at_unix_ms: u64,
    pub reason: PrivateFileGrantRevocationReasonV1,
}

/// Holds the project grant-store lock while an authorized result is applied or
/// a private chunk is transmitted. Revocation uses the same lock.
pub(crate) struct AuthorizedPrivateFileGrantUse {
    _lock: Option<std::fs::File>,
}

pub(crate) fn lock_authorized_grants(
    root: &Path,
    grants: &[RemotePrivateFileGrantV1],
    now_unix_ms: u64,
) -> anyhow::Result<AuthorizedPrivateFileGrantUse> {
    let Some(first) = grants.first() else {
        return Ok(AuthorizedPrivateFileGrantUse { _lock: None });
    };
    let project_id = &first.workspace.project_id;
    anyhow::ensure!(
        grants
            .iter()
            .all(|grant| grant.workspace.project_id == *project_id),
        "private file grants span multiple projects"
    );
    let path = store::path(root, project_id)?;
    let lock = store::lock(&path)?;
    for grant in grants {
        require_authorized(root, grant, now_unix_ms)?;
    }
    Ok(AuthorizedPrivateFileGrantUse { _lock: Some(lock) })
}

/// Return active, metadata-only grants for one task in stable display order.
pub(crate) fn active_for_task(
    root: &Path,
    project_id: &str,
    task_id: &str,
    now_unix_ms: u64,
) -> anyhow::Result<Vec<RemotePrivateFileGrantV1>> {
    crate::execution::contract::validate_identifier(project_id)?;
    crate::execution::contract::validate_identifier(task_id)?;
    let path = store::path(root, project_id)?;
    let state = store::load(project_id, &path)?;
    let revoked = state
        .revocations
        .iter()
        .map(|receipt| receipt.grant_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut grants = state
        .grants
        .into_iter()
        .filter(|grant| {
            grant.workspace.task_id == task_id
                && grant.validate_at(now_unix_ms).is_ok()
                && grant.expires_at_unix_ms > now_unix_ms
                && !revoked.contains(grant.grant_id.as_str())
        })
        .collect::<Vec<_>>();
    grants.sort_by(|left, right| left.grant_id.cmp(&right.grant_id));
    Ok(grants)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivateFileGrantRevocationReasonV1 {
    OperatorRevoked,
    WorkerIdentityChanged,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct GrantStoreV1 {
    schema_version: u8,
    project_id: String,
    grants: Vec<RemotePrivateFileGrantV1>,
    revocations: Vec<PrivateFileGrantRevocationV1>,
}

impl GrantStoreV1 {
    fn empty(project_id: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            project_id: project_id.to_owned(),
            grants: Vec::new(),
            revocations: Vec::new(),
        }
    }

    fn validate(&self, project_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == SCHEMA_VERSION && self.project_id == project_id,
            "private file grant store identity is invalid"
        );
        anyhow::ensure!(self.grants.len() <= MAX_GRANTS);
        let mut grant_ids = std::collections::HashSet::new();
        for grant in &self.grants {
            grant.validate_at(grant.confirmed_at_unix_ms)?;
            anyhow::ensure!(grant.workspace.project_id == project_id);
            anyhow::ensure!(grant_ids.insert(grant.grant_id.as_str()));
        }
        let mut revoked_ids = std::collections::HashSet::new();
        for revoked in &self.revocations {
            let Some(grant) = self
                .grants
                .iter()
                .find(|grant| grant.grant_id == revoked.grant_id)
            else {
                anyhow::bail!("private file grant revocation has no grant receipt");
            };
            anyhow::ensure!(revoked.revoked_at_unix_ms >= grant.confirmed_at_unix_ms);
            anyhow::ensure!(revoked_ids.insert(revoked.grant_id.as_str()));
        }
        Ok(())
    }
}

/// Persist one explicit user-confirmed grant. Repeating the exact receipt is
/// idempotent; reusing a grant ID for different authority is rejected.
pub(crate) fn record_user_confirmation(
    root: &Path,
    confirmed: UserConfirmedPrivateFileGrantV1,
    excluded_paths: &[String],
) -> anyhow::Result<RemotePrivateFileGrantV1> {
    let grant = confirmed.grant;
    let project_id = &grant.workspace.project_id;
    grant.validate_at(crate::persistence::harness_settings::current_unix_ms())?;
    grant.validate_against_exclusions(excluded_paths)?;
    let path = store::path(root, project_id)?;
    let _lock = store::lock(&path)?;
    let mut state = store::load(project_id, &path)?;
    if let Some(existing) = state
        .grants
        .iter()
        .find(|item| item.grant_id == grant.grant_id)
    {
        anyhow::ensure!(
            existing == &grant,
            "private file grant ID conflicts with a prior receipt"
        );
        anyhow::ensure!(
            !state
                .revocations
                .iter()
                .any(|item| item.grant_id == grant.grant_id),
            "private file grant was revoked"
        );
        return Ok(existing.clone());
    }
    anyhow::ensure!(
        state.grants.len() < MAX_GRANTS,
        "private file grant store is full"
    );
    state.grants.push(grant.clone());
    store::write(&path, &state)?;
    Ok(grant)
}

/// Return the exact active receipt only when all bound fields match and the
/// grant remains unexpired and unrevoked.
pub(crate) fn require_authorized(
    root: &Path,
    expected: &RemotePrivateFileGrantV1,
    now_unix_ms: u64,
) -> anyhow::Result<RemotePrivateFileGrantV1> {
    expected.validate_at(now_unix_ms)?;
    let path = store::path(root, &expected.workspace.project_id)?;
    let state = store::load(&expected.workspace.project_id, &path)?;
    let grant = state
        .grants
        .iter()
        .find(|grant| grant.grant_id == expected.grant_id)
        .ok_or_else(|| anyhow::anyhow!("private file grant is not recorded"))?;
    anyhow::ensure!(
        grant == expected,
        "private file grant binding does not match receipt"
    );
    anyhow::ensure!(
        !state
            .revocations
            .iter()
            .any(|revocation| revocation.grant_id == grant.grant_id),
        "private file grant is revoked"
    );
    Ok(grant.clone())
}

/// Load the immutable metadata receipt even when it has expired or been
/// revoked, so recovery can identify and stop its associated execution.
pub(crate) fn receipt_by_id(
    root: &Path,
    project_id: &str,
    grant_id: &str,
) -> anyhow::Result<RemotePrivateFileGrantV1> {
    crate::execution::contract::validate_identifier(project_id)?;
    let parsed = uuid::Uuid::parse_str(grant_id)?;
    anyhow::ensure!(parsed.hyphenated().to_string() == grant_id);
    let path = store::path(root, project_id)?;
    let _lock = store::lock(&path)?;
    let state = store::load(project_id, &path)?;
    state
        .grants
        .into_iter()
        .find(|grant| grant.grant_id == grant_id)
        .ok_or_else(|| anyhow::anyhow!("private file grant receipt is unavailable"))
}

/// Record revocation idempotently. The remote worker must separately stop the
/// execution and verify private-file cleanup before acknowledging this state.
pub(crate) fn revoke(
    root: &Path,
    project_id: &str,
    grant_id: &str,
    now_unix_ms: u64,
    reason: PrivateFileGrantRevocationReasonV1,
) -> anyhow::Result<PrivateFileGrantRevocationV1> {
    let path = store::path(root, project_id)?;
    let _lock = store::lock(&path)?;
    let mut state = store::load(project_id, &path)?;
    let grant = state
        .grants
        .iter()
        .find(|grant| grant.grant_id == grant_id)
        .ok_or_else(|| anyhow::anyhow!("private file grant is not recorded"))?;
    anyhow::ensure!(now_unix_ms >= grant.confirmed_at_unix_ms);
    if let Some(existing) = state
        .revocations
        .iter()
        .find(|item| item.grant_id == grant_id)
    {
        return Ok(existing.clone());
    }
    let receipt = PrivateFileGrantRevocationV1 {
        grant_id: grant_id.to_owned(),
        revoked_at_unix_ms: now_unix_ms,
        reason,
    };
    state.revocations.push(receipt.clone());
    store::write(&path, &state)?;
    Ok(receipt)
}
