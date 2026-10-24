//! Explicit, one-use machine-repair consent and redacted audit journal.
//!
//! Tickets only exist in memory. Restarting Kool.ad/e invalidates them.
//! Approval is NEVER a CLI/LLM tool and this module contains no executor.
//! The app UI must supply a real user-click approval and a fresh, controller-
//! generated host/probe binding; an agent must never construct either.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::bootstrap_repair_policy::{self, Host, Operation, Proposal};

const VERSION: u8 = 1;
const MAX_AUDIT_BYTES: u64 = 512 * 1024;
const MAX_EVENTS: usize = 256;
const CONSENT_TTL_MS: u64 = 10 * 60 * 1000;
const SCAN_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineBinding {
    pub host: Host,
    /// Controller-derived hash of the local machine, never a plain hostname.
    pub machine_hash: String,
    /// Controller-derived hash of the logged-in account.
    pub principal_hash: String,
    /// Controller-derived SHA-256 of the independently probed readiness state.
    pub probe_hash: String,
    pub probed_at_unix_ms: u64,
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

impl MachineBinding {
    fn validate(&self, now_ms: u64) -> anyhow::Result<()> {
        anyhow::ensure!(
            is_sha256_hex(&self.machine_hash)
                && is_sha256_hex(&self.principal_hash)
                && is_sha256_hex(&self.probe_hash),
            "Machine approval must use opaque SHA-256 identity and probe hashes"
        );
        anyhow::ensure!(
            now_ms >= self.probed_at_unix_ms && now_ms - self.probed_at_unix_ms <= SCAN_TTL_MS,
            "Machine readiness evidence is stale; rescan before approving"
        );
        Ok(())
    }
}

fn fingerprint<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Debug)]
struct Pending {
    proposal_digest: String,
    binding_digest: String,
    expires_at_unix_ms: u64,
    operation: Operation,
}

/// A non-cloneable capability consumed by a trusted operation executor.
/// Private fields prevent callers from forging approval by constructing one.
#[derive(Debug)]
pub struct ApprovedAction {
    operation: Operation,
    prerequisite_id: String,
    image_profile: Option<String>,
}

impl ApprovedAction {
    pub fn operation(&self) -> Operation {
        self.operation
    }

    pub fn prerequisite_id(&self) -> &str {
        &self.prerequisite_id
    }

    pub fn image_profile(&self) -> Option<&str> {
        self.image_profile.as_deref()
    }
}

/// An in-memory, controller-owned one-use approval gate.
///
/// Keep this object in the application controller, not a persisted or model-
/// accessible task store. No ticket survives application restart.
pub struct ApprovalGate {
    approvals: HashMap<Uuid, Pending>,
    audit_path: PathBuf,
}

impl ApprovalGate {
    pub fn new() -> Self {
        Self::at_path(
            crate::persistence::state_root()
                .join("machine-setup")
                .join("repair-approval-audit.json"),
        )
    }

    pub fn at_path(audit_path: PathBuf) -> Self {
        Self {
            approvals: HashMap::new(),
            audit_path,
        }
    }

    /// Call only from a genuine confirmation interaction, never from an
    /// agent suggestion or background probe. A declined mutation fails closed.
    pub fn approve_after_user_click(
        &mut self,
        proposal: &Proposal,
        binding: &MachineBinding,
        user_confirmed: bool,
        now_ms: u64,
    ) -> anyhow::Result<Uuid> {
        let plan = bootstrap_repair_policy::validate(proposal, binding.host)?;
        anyhow::ensure!(
            plan.requires_consent,
            "Read-only operations do not require repair approval"
        );
        binding.validate(now_ms)?;
        anyhow::ensure!(
            proposal.host == binding.host,
            "Host identity changed during approval"
        );
        let digest = fingerprint(proposal)?;
        let binding_digest = fingerprint(binding)?;
        if !user_confirmed {
            write_audit(
                &self.audit_path,
                AuditEntry::new(now_ms, &digest, &binding_digest, "declined"),
            )?;
            anyhow::bail!("The user declined the requested machine repair");
        }
        // Never return a usable ticket if its audit receipt cannot be saved.
        write_audit(
            &self.audit_path,
            AuditEntry::new(now_ms, &digest, &binding_digest, "approved"),
        )?;
        let id = Uuid::new_v4();
        self.approvals.insert(
            id,
            Pending {
                proposal_digest: digest,
                binding_digest,
                expires_at_unix_ms: now_ms.saturating_add(CONSENT_TTL_MS),
                operation: plan.operation,
            },
        );
        Ok(id)
    }

    /// Consume before any mutating operation, even on rejection. A second
    /// invocation, new host identity, new model proposal or new probe state
    /// cannot replay the original user approval.
    pub fn consume(
        &mut self,
        id: Uuid,
        proposal: &Proposal,
        current: &MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<ApprovedAction> {
        let pending = self.approvals.remove(&id).ok_or_else(|| {
            anyhow::anyhow!("Machine repair authorization is missing or already used")
        })?;
        let valid = current.validate(now_ms).is_ok()
            && now_ms <= pending.expires_at_unix_ms
            && proposal.host == current.host
            && fingerprint(proposal).as_deref().ok() == Some(pending.proposal_digest.as_str())
            && fingerprint(current).as_deref().ok() == Some(pending.binding_digest.as_str())
            && bootstrap_repair_policy::validate(proposal, current.host)
                .is_ok_and(|plan| plan.requires_consent && plan.operation == pending.operation);
        write_audit(
            &self.audit_path,
            AuditEntry::new(
                now_ms,
                &pending.proposal_digest,
                &pending.binding_digest,
                if valid { "consumed" } else { "rejected" },
            ),
        )?;
        anyhow::ensure!(
            valid,
            "Machine repair consent expired or no longer matches the verified configuration"
        );
        Ok(ApprovedAction {
            operation: pending.operation,
            prerequisite_id: proposal.prerequisite_id.clone(),
            image_profile: proposal.image_profile.clone(),
        })
    }

    pub fn cancel(&mut self, id: Uuid, now_ms: u64) -> anyhow::Result<()> {
        if let Some(pending) = self.approvals.remove(&id) {
            write_audit(
                &self.audit_path,
                AuditEntry::new(
                    now_ms,
                    &pending.proposal_digest,
                    &pending.binding_digest,
                    "cancelled",
                ),
            )?;
        }
        Ok(())
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditEntry {
    observed_at_unix_ms: u64,
    proposal_sha256: String,
    binding_sha256: String,
    outcome: String,
}

impl AuditEntry {
    fn new(at: u64, proposal: &str, binding: &str, outcome: &str) -> Self {
        Self {
            observed_at_unix_ms: at,
            proposal_sha256: proposal.into(),
            binding_sha256: binding.into(),
            outcome: outcome.into(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditStore {
    schema_version: u8,
    entries: Vec<AuditEntry>,
}

fn write_audit(path: &Path, entry: AuditEntry) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid machine audit path"))?;
    fs::create_dir_all(parent)?;
    let parent_meta = fs::symlink_metadata(parent)?;
    anyhow::ensure!(
        parent_meta.is_dir() && !parent_meta.file_type().is_symlink(),
        "Machine audit directory is not a regular directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let lock_path = path.with_extension("lock");
    if let Ok(meta) = fs::symlink_metadata(&lock_path) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Unsafe machine audit lock"
        );
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock: File = options.open(&lock_path)?;
    anyhow::ensure!(
        fs::symlink_metadata(&lock_path)?.is_file(),
        "Unsafe machine audit lock"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match fs2::FileExt::try_lock_exclusive(&lock) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Another instance is recording a machine setup operation"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut store = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AuditStore {
            schema_version: VERSION,
            entries: Vec::new(),
        },
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_AUDIT_BYTES,
                "Machine audit file is unsafe or oversized"
            );
            let stored: AuditStore = serde_json::from_slice(&fs::read(path)?)?;
            anyhow::ensure!(
                stored.schema_version == VERSION,
                "Unknown machine audit version"
            );
            stored
        }
        Err(error) => return Err(error.into()),
    };
    store.entries.push(entry);
    if store.entries.len() > MAX_EVENTS {
        store.entries.drain(..store.entries.len() - MAX_EVENTS);
    }
    let data = serde_json::to_vec(&store)?;
    anyhow::ensure!(
        data.len() as u64 <= MAX_AUDIT_BYTES,
        "Machine audit store is too large"
    );
    crate::artifacts::atomic_write_bytes(path, &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> MachineBinding {
        MachineBinding {
            host: Host::Linux,
            machine_hash: "a".repeat(64),
            principal_hash: "b".repeat(64),
            probe_hash: "c".repeat(64),
            probed_at_unix_ms: 1000,
        }
    }

    fn proposal() -> Proposal {
        Proposal {
            schema_version: 1,
            operation: Operation::PrepareLockedImage,
            host: Host::Linux,
            prerequisite_id: "setup:images:general:amd64".into(),
            image_profile: Some("general".into()),
        }
    }

    fn gate() -> (tempfile::TempDir, ApprovalGate) {
        let dir = tempfile::tempdir().unwrap();
        let gate = ApprovalGate::at_path(dir.path().join("machine").join("audit.json"));
        (dir, gate)
    }

    #[test]
    fn exact_approval_is_one_use_and_does_not_survive_restart() {
        let (_, mut gate) = gate();
        let proposal = proposal();
        let binding = binding();
        let id = gate
            .approve_after_user_click(&proposal, &binding, true, 1100)
            .unwrap();
        let approved = gate.consume(id, &proposal, &binding, 1200).unwrap();
        assert_eq!(approved.operation, Operation::PrepareLockedImage);
        assert!(gate.consume(id, &proposal, &binding, 1201).is_err());
    }

    #[test]
    fn cannot_replay_after_machine_or_probe_change() {
        let (_, mut gate) = gate();
        let id = gate
            .approve_after_user_click(&proposal(), &binding(), true, 1100)
            .unwrap();
        let mut changed = binding();
        changed.probe_hash = "d".repeat(64);
        assert!(gate.consume(id, &proposal(), &changed, 1200).is_err());
        assert!(gate.consume(id, &proposal(), &binding(), 1200).is_err());
    }

    #[test]
    fn rejects_agent_changed_proposal_and_expired_or_stale_evidence() {
        let (_, mut gate) = gate();
        let id = gate
            .approve_after_user_click(&proposal(), &binding(), true, 1100)
            .unwrap();
        let mut changed = proposal();
        changed.image_profile = Some("rust".into());
        assert!(gate.consume(id, &changed, &binding(), 1200).is_err());
        assert!(
            gate.approve_after_user_click(&proposal(), &binding(), true, 301002)
                .is_err()
        );
    }

    #[test]
    fn user_decline_never_grants_authority() {
        let (_, mut gate) = gate();
        assert!(
            gate.approve_after_user_click(&proposal(), &binding(), false, 1200)
                .is_err()
        );
        assert!(gate.approvals.is_empty());
    }

    #[test]
    fn read_only_or_wrong_os_cannot_obtain_mutation_ticket() {
        let (_, mut gate) = gate();
        let mut nonmutating = proposal();
        nonmutating.operation = Operation::RecheckPrerequisites;
        nonmutating.image_profile = None;
        assert!(
            gate.approve_after_user_click(&nonmutating, &binding(), true, 1200)
                .is_err()
        );
        let mut wrong_host = proposal();
        wrong_host.host = Host::Windows;
        assert!(
            gate.approve_after_user_click(&wrong_host, &binding(), true, 1200)
                .is_err()
        );
    }

    #[test]
    fn audit_is_bounded_redacted_and_fails_closed_on_corruption() {
        let (dir, mut gate) = gate();
        let proposal = proposal();
        let binding = binding();
        let _ticket = gate
            .approve_after_user_click(&proposal, &binding, true, 1100)
            .unwrap();
        let content = fs::read_to_string(dir.path().join("machine/audit.json")).unwrap();
        assert!(content.contains("approved"));
        assert!(!content.contains("setup:images:general:amd64"));
        fs::write(dir.path().join("machine/audit.json"), "{broken").unwrap();
        assert!(
            gate.approve_after_user_click(&proposal, &binding, true, 1200)
                .is_err()
        );
    }
}
