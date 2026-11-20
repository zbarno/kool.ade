//! Provider runtime session skeleton (issue zbarno/kool.ade#591, phase 2 of
//! `docs/provider-execution-containment-v1.md`).
//!
//! Defines the container spec the application-owned provider runtime must
//! run under, the pinned-image discipline, and the teardown/kill contract.
//! Everything in this module is inert by construction: with no pinned image
//! the [`RefusingUnpinnedExecutor`] refuses every mutation, so shipping this
//! skeleton cannot move any deployment closer to running providers in a
//! container until the pinned-image ops step lands.

use std::collections::BTreeSet;

/// Container name prefix. Sessions may only manage containers carrying it.
pub const SESSION_NAME_PREFIX: &str = "koolade-provider-";

/// Maximum container-name length accepted by the builder and honored by the
/// teardown ownership gate (kept short for logs and labels; Docker allows
/// 255). Single source of truth so adapters can budget suffixes exactly.
pub const SESSION_MAX_CONTAINER_NAME_LEN: usize = 63;

/// Label marking a container as owned by the provider runtime session.
pub const OWNERSHIP_LABEL_KEY: &str = "com.koolade.provider-runtime";
pub const OWNERSHIP_LABEL_VALUE: &str = "v1";

/// Session resource budget. Values are compile-time constants: no runtime
/// caller may loosen them.
pub const SESSION_PIDS_LIMIT: u64 = 64;
pub const SESSION_MEMORY_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
/// 250ms CPU per 100ms period ~= 2.5 cores ceiling.
pub const SESSION_CPU_PERIOD_US: u64 = 100_000;
pub const SESSION_CPU_QUOTA_US: u64 = 250_000;

/// Host paths the provider container may never receive as a bind mount,
/// checked against the literal mount source (symlinks are resolved by the
/// caller before validation).
pub const DENIED_HOST_MOUNT_SOURCES: &[&str] = &[
    "/",
    "/dev",
    "/etc",
    "/boot",
    "/proc",
    "/proc/sys",
    "/sys",
    "/root",
    "/home",
    "/mnt",
    "/media",
    "/run",
    "/var/run",
    "/var/run/docker.sock",
    "/var/run/containerd.sock",
    "/var/lib/docker",
    "/var/lib/containerd",
];

/// Whitelisted tmpfs destinations. Nothing else may be tmp-mounted.
pub const ALLOWED_TMPFS_DESTINATIONS: &[&str] = &["/tmp", "/run"];

/// Failure of a provider session operation. Fail-closed by construction:
/// every variant means "do nothing / stay restricted".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSessionError {
    /// No provider runtime image is pinned yet; all mutations refused.
    ImageNotPinned,
    /// A proposed mount source is on the denied host-path list.
    DeniedHostMountSource(String),
    /// A proposed tmpfs destination is not whitelisted.
    DisallowedTmpfsDestination(String),
    /// Proposed container name violates the ownership prefix.
    /// A whitelisted tmpfs destination was requested with zero size.
    InvalidTmpfsSize(String),
    InvalidContainerName(String),
    /// The session tried to manage a container it does not own.
    OwnershipMismatch,
    /// Exec requested before a container is known-started.
    ExecBeforeStart,
    /// Termination ran but the container has not verifiably disappeared.
    /// Container survived termination: a verified-absent gate failed.
    TerminatedWithResidue(String),
    /// Engine terminate call errored while the container still presented.
    TerminateFailed {
        name: String,
        cause: String,
    },
    /// Pinned image reference/digest failed format validation.
    InvalidImagePin(String),
}

impl std::fmt::Display for ProviderSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            ProviderSessionError::ImageNotPinned => {
                "provider runtime image is not pinned yet; refusing mutation".to_string()
            }
            ProviderSessionError::DeniedHostMountSource(path) => {
                format!("bind mount source {path} is denied for provider containers")
            }
            ProviderSessionError::DisallowedTmpfsDestination(dest) => {
                format!("tmpfs destination {dest} is not whitelisted for provider containers")
            }
            ProviderSessionError::InvalidTmpfsSize(dest) => {
                format!("tmpfs size for whitelisted destination {dest} must be nonzero")
            }
            ProviderSessionError::InvalidContainerName(name) => format!(
                "container name {name:?} violates the {} ownership prefix",
                SESSION_NAME_PREFIX
            ),
            ProviderSessionError::OwnershipMismatch => {
                "refusing to manage a container without our ownership label".to_string()
            }
            ProviderSessionError::ExecBeforeStart => {
                "refusing exec before the container has verifiably started".to_string()
            }
            ProviderSessionError::TerminatedWithResidue(name) => {
                format!("container {name} did not verifiably disappear after termination")
            }
            ProviderSessionError::TerminateFailed { name, cause } => format!(
                "terminate call failed for {name} while it still presented (cause: {cause})"
            ),
            ProviderSessionError::InvalidImagePin(detail) => {
                format!("provider image pin rejected: {detail}")
            }
        };
        f.write_str(&msg)
    }
}

/// One declared mount in the container spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredMount {
    pub kind: MountKind,
    /// For tmpfs mounts this is the destination; unused otherwise.
    pub tmpfs_destination: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountKind {
    /// Tmpfs scratch with a sized upper bound. Size is fixed per whitelist
    /// entry; not negotiable.
    TmpfsScratch { size_bytes: u64 },
}

/// Fully validated, immutable container spec. Produced by
/// [`ProviderSpecBuilder::build`], which pins every unsafe knob to the
/// compile-time constants above. Fields are `pub` for the upcoming executor
/// port slice; tightening them to private accessors lands WITH the wiring
/// slice so the builder remains the only construction path in practice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderContainerSpec {
    pub image_reference: String,
    pub image_digest: String,
    pub container_name: String,
    pub readonly_rootfs: bool,
    pub no_new_privileges: bool,
    pub dropped_capabilities: BTreeSet<String>,
    pub added_capabilities: BTreeSet<String>,
    pub mounts: Vec<DeclaredMount>,
    pub pids_limit: u64,
    pub memory_limit_bytes: u64,
    pub cpu_period_us: u64,
    pub cpu_quota_us: u64,
    pub network: ProviderNetworkMode,
}

/// Network posture placeholders. The v1 default refuses to define a network
/// mode until the phase-2a egress spike freezes the contract; anything else
/// is an error until then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderNetworkMode {
    /// No network mode has been authorized yet.
    Undecided,
}

impl ProviderContainerSpec {
    /// True when this spec is fully sealed against the constants.
    pub fn is_sealed(&self) -> bool {
        self.readonly_rootfs
            && self.no_new_privileges
            && self.added_capabilities.is_empty()
            && self.dropped_capabilities.contains("ALL")
            && self.pids_limit == SESSION_PIDS_LIMIT
            && self.memory_limit_bytes == SESSION_MEMORY_LIMIT_BYTES
            && self.cpu_period_us == SESSION_CPU_PERIOD_US
            && self.cpu_quota_us == SESSION_CPU_QUOTA_US
    }
}

/// Validates a proposed host mount source against the denied list. Callers
/// must pass the *resolved* source path (realpath) so symlink tricks cannot
/// smuggle a denied source past the literal check.
pub fn validate_host_mount_source(resolved_source: &str) -> Result<(), ProviderSessionError> {
    let source = resolved_source.trim();
    if source.is_empty() || !source.starts_with('/') {
        return Err(ProviderSessionError::DeniedHostMountSource(
            source.to_string(),
        ));
    }
    for denied in DENIED_HOST_MOUNT_SOURCES {
        if source == *denied
            || source
                .strip_prefix(denied)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            return Err(ProviderSessionError::DeniedHostMountSource(
                source.to_string(),
            ));
        }
    }
    Ok(())
}

/// Validates a tmpfs destination against the whitelist.
pub fn validate_tmpfs_destination(destination: &str) -> Result<(), ProviderSessionError> {
    if ALLOWED_TMPFS_DESTINATIONS.contains(&destination) {
        Ok(())
    } else {
        Err(ProviderSessionError::DisallowedTmpfsDestination(
            destination.to_string(),
        ))
    }
}

/// Builds [`ProviderContainerSpec`] values. All safety knobs are fixed to
/// the module constants; the builder cannot express weaker settings.
#[derive(Clone, Debug, Default)]
pub struct ProviderSpecBuilder {
    image_reference: String,
    image_digest: String,
    container_name: String,
    mounts: Vec<DeclaredMount>,
}

impl ProviderSpecBuilder {
    /// Starts a builder for the given (reference, digest) image pin. Pin
    /// FORM is validated here; pin AUTHORIZATION happens later via the lock.
    pub fn new(image_reference: impl Into<String>, image_digest: impl Into<String>) -> Self {
        Self {
            image_reference: image_reference.into(),
            image_digest: image_digest.into(),
            ..Self::default()
        }
    }

    /// Sets the (prefix-required) container name; validated at `build()`.
    pub fn container_name(mut self, name: impl Into<String>) -> Self {
        self.container_name = name.into();
        self
    }

    /// Adds a whitelisted tmpfs scratch mount. The only mount kind the v1
    /// spec permits: host bind mounts are categorically excluded from the
    /// builder API.
    pub fn add_scratch_tmpfs(
        self,
        destination: &str,
        size_bytes: u64,
    ) -> Result<Self, ProviderSessionError> {
        validate_tmpfs_destination(destination)?;
        if size_bytes == 0 {
            return Err(ProviderSessionError::InvalidTmpfsSize(
                destination.to_string(),
            ));
        }
        let mut mounts = self.mounts.clone();
        mounts.push(DeclaredMount {
            kind: MountKind::TmpfsScratch { size_bytes },
            tmpfs_destination: Some(destination.to_string()),
        });
        Ok(Self { mounts, ..self })
    }

    /// Validates the accumulated state and seals the spec. All limits
    /// resolve to the module constants; the caller cannot weaken them.
    pub fn build(self) -> Result<ProviderContainerSpec, ProviderSessionError> {
        let name = self.container_name;
        if !name.starts_with(SESSION_NAME_PREFIX) {
            return Err(ProviderSessionError::InvalidContainerName(name));
        }
        // Conservative cap aligned to common identifier limits (Docker
        // allows 255; we keep names short for logs and labels).
        if name.len() > SESSION_MAX_CONTAINER_NAME_LEN {
            return Err(ProviderSessionError::InvalidContainerName(name));
        }
        Ok(ProviderContainerSpec {
            image_reference: self.image_reference,
            image_digest: self.image_digest,
            container_name: name,
            readonly_rootfs: true,
            no_new_privileges: true,
            dropped_capabilities: BTreeSet::from(["ALL".to_string()]),
            added_capabilities: BTreeSet::new(),
            mounts: self.mounts,
            pids_limit: SESSION_PIDS_LIMIT,
            memory_limit_bytes: SESSION_MEMORY_LIMIT_BYTES,
            cpu_period_us: SESSION_CPU_PERIOD_US,
            cpu_quota_us: SESSION_CPU_QUOTA_US,
            network: ProviderNetworkMode::Undecided,
        })
    }
}

/// A pinned, format-validated provider image identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedProviderImage {
    pub reference: String,
    pub digest: String,
}

/// Strict pin validation: registry-scoped reference pinned BY DIGEST only
/// (tag-only references drift), digest must be exactly `sha256:` + 64
/// lowercase hex, and local registries are refused.
pub fn validate_image_pin(
    reference: &str,
    digest: &str,
) -> Result<PinnedProviderImage, ProviderSessionError> {
    let rej = |detail: &str| ProviderSessionError::InvalidImagePin(detail.into());
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(rej("empty reference"));
    }
    if reference.ends_with(':') {
        return Err(rej("reference ends with ':': trailing separator"));
    }
    if reference.contains("://") {
        return Err(rej("explicit schemes are not allowed in image references"));
    }
    let digest = digest.trim();
    let hash_hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| rej("digest must be prefixed with sha256:"))?;
    if hash_hex.len() != 64
        || !hash_hex
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(rej(
            "digest hash must be exactly 64 lowercase hex characters",
        ));
    }
    let repo_part = match reference.rsplit_once('@') {
        Some((repo, embedded_digest)) => {
            // The lock's canonical form embeds the SCHEMED digest
            // (`repo@sha256:<hex>`); normalize before comparing.
            let embedded_bare = embedded_digest
                .strip_prefix("sha256:")
                .unwrap_or(embedded_digest);
            if embedded_bare != hash_hex {
                return Err(rej("embedded digest differs from the pinned digest"));
            }
            repo
        }
        None => reference,
    };
    if repo_part.is_empty() || repo_part.starts_with("localhost/") || repo_part.contains(":5000/") {
        return Err(rej(
            "local registries are not authorized provider image sources",
        ));
    }
    Ok(PinnedProviderImage {
        reference: reference.to_string(),
        digest: digest.to_string(),
    })
}

/// Port over which sessions drive the container engine. Kept behind a trait
/// so the skeleton is testable without a live daemon; production adapters
/// land in a later slice.
pub trait ProviderExecutorPort {
    fn create(&mut self, spec: &ProviderContainerSpec) -> Result<(), ProviderSessionError>;
    fn exec(&mut self, name: &str, argv: &[String]) -> Result<i32, ProviderSessionError>;
    fn terminate(&mut self, name: &str) -> Result<(), ProviderSessionError>;
    fn verify_absent(&mut self, name: &str) -> bool;
    fn remove(&mut self, name: &str) -> Result<(), ProviderSessionError>;
}

/// Default executor: refuses everything until an image is pinned. This makes
/// the merged skeleton provably inert: every mutation endpoint errors with
/// [`ProviderSessionError::ImageNotPinned`].
#[derive(Default)]
pub struct RefusingUnpinnedExecutor;

impl ProviderExecutorPort for RefusingUnpinnedExecutor {
    fn create(&mut self, _: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
        Err(ProviderSessionError::ImageNotPinned)
    }
    fn exec(&mut self, _: &str, _: &[String]) -> Result<i32, ProviderSessionError> {
        Err(ProviderSessionError::ImageNotPinned)
    }
    fn terminate(&mut self, _: &str) -> Result<(), ProviderSessionError> {
        Err(ProviderSessionError::ImageNotPinned)
    }
    fn verify_absent(&mut self, _: &str) -> bool {
        // Unknown is not "present": refuse, but cleanup must re-check.
        false
    }
    fn remove(&mut self, _: &str) -> Result<(), ProviderSessionError> {
        Err(ProviderSessionError::ImageNotPinned)
    }
}

/// Recorded outcome of a teardown/kill pass. `ok` is true only when the
/// container is *verified* absent; residue is always surfaced, never
/// absorbed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminationRecord {
    pub container_name: String,
    pub terminate_requested: bool,
    pub verified_absent: bool,
    pub removed: bool,
    pub idempotent_already_absent: bool,
}

impl TerminationRecord {
    /// The arbitrating gate is verified absence; every successful record
    /// (whether torn-down or already gone) satisfies it.
    pub fn ok(&self) -> bool {
        self.verified_absent
    }
}

/// Runs the teardown/kill contract against `port` for one container.
///
/// Contract (negative-tested in CI):
/// 1. The container name must carry [`SESSION_NAME_PREFIX`] or no port call
///    is made at all ([`ProviderSessionError::OwnershipMismatch`]).
/// 2. Pre-check: if already absent, short-circuit as idempotent success
///    WITHOUT any destructive call.
/// 3. Otherwise terminate, then verify absent, then remove, then verify
///    absent again. Success requires the final verified-absent observation;
///    any residue yields [`ProviderSessionError::TerminatedWithResidue`].
pub fn terminate_then_cleanup(
    port: &mut dyn ProviderExecutorPort,
    name: &str,
) -> Result<TerminationRecord, ProviderSessionError> {
    if !name.starts_with(SESSION_NAME_PREFIX) {
        return Err(ProviderSessionError::OwnershipMismatch);
    }
    if port.verify_absent(name) {
        return Ok(TerminationRecord {
            container_name: name.to_string(),
            terminate_requested: false,
            verified_absent: true,
            removed: false,
            idempotent_already_absent: true,
        });
    }
    match port.terminate(name) {
        Ok(()) => {}
        Err(err) => {
            // A terminate error on an already-gone container is benign only
            // if the verified-absent gate confirms it. ImageNotPinned keeps
            // its identity so the pre-pin refusal stays diagnosable.
            if port.verify_absent(name) {
                return Ok(TerminationRecord {
                    container_name: name.to_string(),
                    terminate_requested: true,
                    verified_absent: true,
                    removed: false,
                    idempotent_already_absent: false,
                });
            }
            return Err(if matches!(err, ProviderSessionError::ImageNotPinned) {
                err
            } else {
                ProviderSessionError::TerminateFailed {
                    name: name.to_string(),
                    cause: err.to_string(),
                }
            });
        }
    }
    if !port.verify_absent(name) {
        return Err(ProviderSessionError::TerminatedWithResidue(
            name.to_string(),
        ));
    }
    let removed = matches!(port.remove(name), Ok(()));
    if !port.verify_absent(name) {
        return Err(ProviderSessionError::TerminatedWithResidue(
            name.to_string(),
        ));
    }
    Ok(TerminationRecord {
        container_name: name.to_string(),
        terminate_requested: true,
        verified_absent: true,
        removed,
        idempotent_already_absent: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Scriptable double that records the exact call sequence.
    #[derive(Default)]
    struct MockPort {
        calls: RefCell<Vec<&'static str>>,
        present: RefCell<bool>,
        fail_remove: bool,
        fail_terminate: bool,
    }
    impl MockPort {
        fn track_present(self) -> Self {
            *self.present.borrow_mut() = true;
            self
        }
        fn call_log(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }
    }
    impl ProviderExecutorPort for MockPort {
        fn create(&mut self, _: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
            self.calls.borrow_mut().push("create");
            Ok(())
        }
        fn exec(&mut self, _: &str, _: &[String]) -> Result<i32, ProviderSessionError> {
            self.calls.borrow_mut().push("exec");
            Ok(0)
        }
        fn terminate(&mut self, _: &str) -> Result<(), ProviderSessionError> {
            self.calls.borrow_mut().push("terminate");
            if self.fail_terminate {
                // Engine call faults; the mock still presents afterward.
                return Err(ProviderSessionError::TerminatedWithResidue(
                    "simulated terminate fault".into(),
                ));
            }
            *self.present.borrow_mut() = false;
            Ok(())
        }
        fn verify_absent(&mut self, _: &str) -> bool {
            self.calls.borrow_mut().push("verify");
            !*self.present.borrow()
        }
        fn remove(&mut self, _: &str) -> Result<(), ProviderSessionError> {
            self.calls.borrow_mut().push("remove");
            if self.fail_remove {
                Err(ProviderSessionError::TerminatedWithResidue(
                    "simulated".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    fn name(suffix: &str) -> String {
        format!("{SESSION_NAME_PREFIX}{suffix}")
    }

    #[test]
    fn unpinned_executor_refuses_every_mutation_endpoint() {
        let spec =
            ProviderSpecBuilder::new("reg.example/img", format!("sha256:{}", "a".repeat(64)))
                .container_name(name("unit"))
                .build()
                .expect("sealed spec");
        let mut port = RefusingUnpinnedExecutor;
        assert_eq!(
            port.create(&spec),
            Err(ProviderSessionError::ImageNotPinned)
        );
        assert_eq!(
            port.exec(&spec.container_name, &["/bin/sh".into()]),
            Err(ProviderSessionError::ImageNotPinned)
        );
        assert_eq!(
            port.terminate(&spec.container_name),
            Err(ProviderSessionError::ImageNotPinned)
        );
        assert_eq!(
            port.remove(&spec.container_name),
            Err(ProviderSessionError::ImageNotPinned)
        );
        assert!(!port.verify_absent(&spec.container_name));
    }

    #[test]
    fn spec_is_sealed_and_constants_are_within_safe_bounds() {
        let spec =
            ProviderSpecBuilder::new("reg.example/img", format!("sha256:{}", "b".repeat(64)))
                .container_name(name("bounds"))
                .build()
                .expect("sealed");
        assert!(spec.is_sealed());
        assert!((32..=128).contains(&spec.pids_limit));
        assert!(spec.memory_limit_bytes <= 4 * 1024 * 1024 * 1024);
        assert!(spec.cpu_quota_us <= 2 * spec.cpu_period_us * 5);
        assert!(spec.added_capabilities.is_empty());
        assert!(matches!(spec.network, ProviderNetworkMode::Undecided));
    }

    #[test]
    fn container_name_must_carry_the_ownership_prefix_and_length_bound() {
        let base =
            ProviderSpecBuilder::new("reg.example/img", format!("sha256:{}", "c".repeat(64)));
        let err = base
            .clone()
            .container_name("someone-elses-box")
            .build()
            .expect_err("reject foreign name");
        assert!(matches!(err, ProviderSessionError::InvalidContainerName(_)));
        let long = "long".repeat(20);
        assert!(
            base.clone()
                .container_name(format!("{SESSION_NAME_PREFIX}{long}"))
                .build()
                .is_err()
        );
    }

    #[test]
    fn denied_host_sources_include_socket_dev_and_roots() {
        for source in [
            "/var/run/docker.sock",
            "/dev",
            "/etc",
            "/var/lib/docker/a/containerd-root",
            "/root",
            "/",
            "/sys",
            "/sys/firmware",
            "/media/usb",
        ] {
            assert!(
                validate_host_mount_source(source).is_err(),
                "expected {source} to be denied"
            );
        }
    }

    #[test]
    fn tmpfs_whitelist_only_allows_declared_destinations() {
        assert!(validate_tmpfs_destination("/tmp").is_ok());
        assert!(validate_tmpfs_destination("/run").is_ok());
        assert!(validate_tmpfs_destination("/etc/tmp").is_err());
        assert!(
            ProviderSpecBuilder::new("img", "sha256:x")
                .add_scratch_tmpfs("/proc", 1024)
                .is_err()
        );
    }

    #[test]
    fn termination_contract_happy_path_preserves_stage_order() {
        let mut port = MockPort::default().track_present();
        let n = name("happy");
        let record = terminate_then_cleanup(&mut port, &n).expect("teardown ok");
        assert!(record.ok());
        assert!(!record.idempotent_already_absent);
        assert_eq!(
            port.call_log(),
            vec!["verify", "terminate", "verify", "remove", "verify"]
        );
    }

    #[test]
    fn termination_is_idempotent_when_already_absent_and_calls_nothing_destructive() {
        let mut port = MockPort::default();
        let n = name("gone");
        let record = terminate_then_cleanup(&mut port, &n).expect("idempotent ok");
        assert!(record.idempotent_already_absent);
        assert_eq!(port.call_log(), vec!["verify"]);
    }

    #[test]
    fn termination_with_residue_fails_closed() {
        let n = name("zombie");
        let mut lying = ResidueLyingPort {
            present: true,
            calls: Vec::new(),
        };
        let err = terminate_then_cleanup(&mut lying, &n).expect_err("residue surfaces");
        assert!(matches!(
            err,
            ProviderSessionError::TerminatedWithResidue(_)
        ));
        assert!(lying.calls.contains(&"terminate"));
        assert!(
            !lying.calls.contains(&"remove"),
            "no remove on residue path"
        );
    }

    #[test]
    fn remove_error_recovers_only_when_final_verify_confirms_absence() {
        let mut failing = MockPort {
            fail_remove: true,
            ..Default::default()
        };
        *failing.present.borrow_mut() = true;
        let n = name("removetail");
        // remove() errors but the container IS verified absent on both
        // verify gates: the arbitrating observation wins, recorded
        // transparently as removed=false.
        let record =
            terminate_then_cleanup(&mut failing, &n).expect("verified absence arbitrates success");
        assert!(record.ok());
        assert!(!record.removed);
    }

    #[test]
    fn ownership_guard_blocks_foreign_names_before_any_port_call() {
        let mut port = MockPort::default();
        let err = terminate_then_cleanup(&mut port, "victim-other-team").expect_err("guard");
        assert_eq!(err, ProviderSessionError::OwnershipMismatch);
        assert!(
            port.call_log().is_empty(),
            "no port calls for foreign names"
        );
    }

    #[test]
    fn image_pins_require_digest_scoped_references_and_clean_hashes() {
        let good = format!("reg.example/koolade/provider-runtime@{}", "d".repeat(64));
        assert!(validate_image_pin(&good, &format!("sha256:{}", "d".repeat(64))).is_ok());
        assert!(
            validate_image_pin(
                "reg.example/koolade/provider-runtime:v1",
                &format!("sha256:{}", "d".repeat(64))
            )
            .is_ok(),
            "bare repository reference plus separate digest is allowed"
        );
        assert!(
            validate_image_pin(&good, &format!("sha256:{}", "e".repeat(64))).is_err(),
            "embedded digest mismatch must be rejected"
        );
        assert!(
            validate_image_pin(&good, &format!("sha256:{}", "D".repeat(64))).is_err(),
            "uppercase hex digest must be rejected"
        );
        assert!(
            validate_image_pin(
                "localhost/koolade/provider-runtime",
                &format!("sha256:{}", "f".repeat(64))
            )
            .is_err()
        );
        assert!(validate_image_pin("reg/example", "sha256:deadbeef").is_err());
        assert!(validate_image_pin("", &format!("sha256:{}", "0".repeat(64))).is_err());
    }

    #[derive(Default)]
    struct ResidueLyingPort {
        present: bool,
        calls: Vec<&'static str>,
    }
    impl ProviderExecutorPort for ResidueLyingPort {
        fn create(&mut self, _: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
            Ok(())
        }
        fn exec(&mut self, _: &str, _: &[String]) -> Result<i32, ProviderSessionError> {
            Ok(0)
        }
        fn terminate(&mut self, _: &str) -> Result<(), ProviderSessionError> {
            // Pretends to terminate but the container stays present.
            self.calls.push("terminate");
            self.present = true;
            Ok(())
        }
        fn verify_absent(&mut self, _: &str) -> bool {
            self.calls.push("verify");
            !self.present
        }
        fn remove(&mut self, _: &str) -> Result<(), ProviderSessionError> {
            self.calls.push("remove");
            Ok(())
        }
    }

    #[test]
    fn tmpfs_zero_size_reports_a_distinct_invalid_size_error() {
        let err = ProviderSpecBuilder::new("img", "sha256:x")
            .add_scratch_tmpfs("/tmp", 0)
            .expect_err("zero size rejected");
        assert!(matches!(
            err,
            ProviderSessionError::InvalidTmpfsSize(ref d) if d.as_str() == "/tmp"
        ));
    }

    #[test]
    fn terminate_engine_fault_reports_failed_with_carried_cause() {
        let mut port = MockPort {
            fail_terminate: true,
            ..Default::default()
        };
        *port.present.borrow_mut() = true;
        let n = name("faultbox");
        let err = terminate_then_cleanup(&mut port, &n).expect_err("fault surfaces");
        match err {
            ProviderSessionError::TerminateFailed { name: ename, cause } => {
                assert_eq!(ename, n);
                assert!(cause.contains("simulate"), "cause carried: {cause}");
            }
            other => panic!("expected TerminateFailed, got {other:?}"),
        }
        let log = port.call_log();
        assert!(log.contains(&"terminate"));
        assert!(!log.contains(&"remove"), "no remove after terminate fault");
    }

    #[test]
    fn lock_form_canonical_pins_pass_validation() {
        // Exact shape used by containers/images.lock.json:
        // `repository@sha256:<hex>` with the SCHEME embedded.
        let d64 = "a".repeat(64);
        let ref_pin = format!("docker.io/library/debian@sha256:{d64}");
        assert!(
            validate_image_pin(&ref_pin, &format!("sha256:{d64}")).is_ok(),
            "canonical lock form must be accepted"
        );
    }
}
