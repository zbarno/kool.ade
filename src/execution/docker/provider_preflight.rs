//! Provider runtime preflight for the coding-provider containment boundary.
//!
//! Evaluates whether a Docker engine snapshot can host the application-owned
//! provider runtime (issue zbarno/kool.ade#591, phase 2 of
//! `docs/provider-execution-containment-v1.md`). The evaluator is pure and
//! fail-closed: it reports every unmet precondition with a plain-English
//! blocker and recovery action rather than stopping at the first one, and
//! `ready` is true only when nothing is missing. Baseline engine
//! prerequisites are delegated to the existing discovery validation so the
//! provider runtime never accepts an engine the trusted execution path
//! itself would reject.

use crate::execution::docker::discovery::{PrerequisiteFailure, validate_snapshot};
use crate::execution::docker::engine::EngineSnapshot;

/// Machine-readable classifier for one unmet provider runtime precondition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProviderPreconditionCode {
    /// The baseline engine prerequisites were not met; the finding message
    /// carries the underlying baseline failure text.
    BaselineNotMet,
    /// Native Linux Docker is running rootfully (or with root ambiguity).
    RootfulOrAmbiguousEngine,
    /// Docker Desktop / packaged-VM engines are outside the v1 scope.
    PackagedVmEngineOutOfScope,
    /// Engine host OS is not Linux.
    NonLinuxEngineHost,
    /// Seccomp filtering is disabled engine-wide, so the containment syscall
    /// posture cannot be applied.
    SeccompUnavailable,
    /// The engine cannot enforce pid-count limits required by the session
    /// budget.
    PidLimitUnavailable,
    /// The engine cannot enforce memory limits required by the session
    /// budget.
    MemoryLimitUnavailable,
    /// The engine cannot enforce CPU quotas required by the session budget.
    CpuQuotaUnavailable,
}

/// One unmet precondition, expressed for user-facing reporting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderPreconditionFinding {
    pub code: ProviderPreconditionCode,
    pub message: String,
    pub retryable: bool,
}

impl ProviderPreconditionFinding {
    fn new(code: ProviderPreconditionCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// Aggregate, fail-closed preflight outcome. `ready` is true iff `findings`
/// is empty; callers must never treat a populated report as usable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderPreflightReport {
    pub findings: Vec<ProviderPreconditionFinding>,
}

impl ProviderPreflightReport {
    pub fn ready(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.ready()
    }

    pub fn has_code(&self, code: ProviderPreconditionCode) -> bool {
        self.findings.iter().any(|finding| finding.code == code)
    }
}

/// Evaluate the provider runtime preconditions against `snapshot`.
///
/// Order of findings is deterministic: baseline failures first, then
/// host-shape gates, then budget/posture gates. Multiple findings are
/// reported so a user can remediate everything in one pass.
pub fn evaluate_provider_runtime_preconditions(
    snapshot: &EngineSnapshot,
) -> ProviderPreflightReport {
    let mut findings: Vec<ProviderPreconditionFinding> = Vec::new();

    if let Err(PrerequisiteFailure {
        message, retryable, ..
    }) = validate_snapshot(snapshot)
    {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::BaselineNotMet,
            format!("Provider runtime baseline not met: {message}"),
            retryable,
        ));
    }

    // Host-shape gates. A packaged VM engine (Docker Desktop) or a
    // non-native-Linux host is out of v1 scope regardless of the baseline
    // result; report shape findings even when the baseline already failed so
    // the user learns the full remediation set.
    if snapshot.os_type != "linux" || !snapshot.native_linux {
        if snapshot.os_type == "linux" {
            findings.push(ProviderPreconditionFinding::new(
                ProviderPreconditionCode::PackagedVmEngineOutOfScope,
                "Provider runtime v1 requires a native Linux host; this Docker engine runs inside a packaged virtual machine, which is not in scope yet.",
                false,
            ));
        } else {
            findings.push(ProviderPreconditionFinding::new(
                ProviderPreconditionCode::NonLinuxEngineHost,
                "Provider runtime v1 requires a Linux Docker engine; macOS and Windows hosts keep their existing fail-closed provider restrictions.",
                false,
            ));
        }
    } else if snapshot.docker_desktop {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::PackagedVmEngineOutOfScope,
            "Provider runtime v1 requires rootless Docker on a native Linux host; Docker Desktop is not in scope yet.",
            false,
        ));
    } else if !snapshot.rootless {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::RootfulOrAmbiguousEngine,
            "Provider runtime v1 requires the rootless Docker setup already used by Kool.ad/e execution; dockerd is running with root authority here. Switch to the user-level (rootless) Docker daemon for this account.",
            false,
        ));
    }

    // Budget/posture gates (independent of host shape so remediation lists
    // stay complete).
    if !snapshot.seccomp_enabled {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::SeccompUnavailable,
            "The Docker engine has seccomp filtering disabled. The provider container's syscall posture requires seccomp; re-enable the engine default and reload dockerd.",
            false,
        ));
    }
    if !snapshot.pids_limit {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::PidLimitUnavailable,
            "The Docker engine cannot enforce pid-count limits. Configure a cgroup driver with pids-controller support (systemd or cgroupfs) so session pid budgets can be applied.",
            false,
        ));
    }
    if !snapshot.memory_limit {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::MemoryLimitUnavailable,
            "The Docker engine cannot enforce memory limits. Configure cgroup memory controls (memory.limit_in_bytes available) so session memory budgets can be applied.",
            false,
        ));
    }
    if !snapshot.cpu_quota {
        findings.push(ProviderPreconditionFinding::new(
            ProviderPreconditionCode::CpuQuotaUnavailable,
            "The Docker engine cannot enforce CPU quotas. Configure cgroup CPU controls (cpu.cfs_quota_us supported) so session CPU budgets can be applied.",
            false,
        ));
    }

    ProviderPreflightReport { findings }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baseline-valid rootless native Linux engine. Individual tests flip
    /// exactly the axes they exercise.
    fn healthy_snapshot() -> EngineSnapshot {
        EngineSnapshot {
            daemon_id: "daemon-unit".into(),
            daemon_version: "28.0.0".into(),
            api_version: "1.51".into(),
            minimum_api_version: "1.24".into(),
            os_type: "linux".into(),
            architecture: "amd64".into(),
            security_options: vec!["name=seccomp,profile=default".into()],
            rootless: true,
            native_linux: true,
            docker_desktop: false,
            memory_limit: true,
            cpu_quota: true,
            pids_limit: true,
            cgroup_version: Some("2".into()),
            cgroup_driver: Some("systemd".into()),
            seccomp_enabled: true,
            docker_storage_accessible: true,
            disk_available_bytes: Some(512 * 1024 * 1024 * 1024),
            minimum_disk_bytes: 10 * 1024 * 1024 * 1024,
        }
    }

    #[test]
    fn healthy_rootless_native_engine_reports_no_findings() {
        let report = evaluate_provider_runtime_preconditions(&healthy_snapshot());
        assert!(
            report.findings.is_empty(),
            "unexpected findings: {report:?}"
        );
        assert!(report.ready());
        assert!(report.is_empty());
    }

    #[test]
    fn non_linux_host_fails_closed_with_platform_reason() {
        let mut snap = healthy_snapshot();
        snap.os_type = "darwin".into();
        snap.native_linux = false;
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(!report.ready());
        assert!(report.has_code(ProviderPreconditionCode::NonLinuxEngineHost));
    }

    #[test]
    fn packaged_vm_engine_is_out_of_scope_even_when_otherwise_valid() {
        let mut snap = healthy_snapshot();
        snap.docker_desktop = true;
        snap.native_linux = false;
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(!report.ready());
        assert!(report.has_code(ProviderPreconditionCode::PackagedVmEngineOutOfScope));
    }

    #[test]
    fn rootful_native_engine_requires_switch_to_rootless() {
        let mut snap = healthy_snapshot();
        snap.rootless = false;
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(!report.ready());
        assert!(report.has_code(ProviderPreconditionCode::RootfulOrAmbiguousEngine));
    }

    #[test]
    fn seccomp_disabled_blocks_the_syscall_posture_gate() {
        let mut snap = healthy_snapshot();
        snap.seccomp_enabled = false;
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(!report.ready());
        assert!(report.has_code(ProviderPreconditionCode::SeccompUnavailable));
    }

    #[test]
    fn each_budget_gate_reports_its_own_recovery_action() {
        let mut snap = healthy_snapshot();
        snap.pids_limit = false;
        snap.memory_limit = false;
        snap.cpu_quota = false;
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(!report.ready());
        // Three specific budget findings plus the mapped baseline finding:
        // the shared engine validator expresses the same axes in aggregate,
        // and both views are reported so remediation stays complete.
        assert_eq!(report.findings.len(), 4);
        assert!(report.has_code(ProviderPreconditionCode::BaselineNotMet));
        assert!(report.has_code(ProviderPreconditionCode::PidLimitUnavailable));
        assert!(report.has_code(ProviderPreconditionCode::MemoryLimitUnavailable));
        assert!(report.has_code(ProviderPreconditionCode::CpuQuotaUnavailable));
        for finding in &report.findings {
            assert!(
                finding.message.chars().count() >= 40,
                "finding lacks a recovery action: {}",
                finding.message
            );
        }
    }

    #[test]
    fn baseline_failures_are_surfaced_not_swallowed() {
        let mut snap = healthy_snapshot();
        snap.os_type = "windows".into();
        let report = evaluate_provider_runtime_preconditions(&snap);
        assert!(report.has_code(ProviderPreconditionCode::BaselineNotMet));
        // Windows containers are also a shape finding; both are reported so
        // remediation is complete in one pass.
        assert!(report.has_code(ProviderPreconditionCode::NonLinuxEngineHost));
        assert!(!report.ready());
    }

    #[test]
    fn findings_are_deterministic_in_order() {
        let mut snap = healthy_snapshot();
        snap.rootless = false;
        snap.seccomp_enabled = false;
        snap.cpu_quota = false;
        let first = evaluate_provider_runtime_preconditions(&snap);
        let second = evaluate_provider_runtime_preconditions(&snap);
        assert_eq!(first, second);
        // Deterministic emission order: baseline, host shape, posture,
        // budgets. Rootful native engines also fail the baseline gate, so
        // the leading finding is the mapped baseline failure.
        let codes: Vec<_> = first.findings.iter().map(|f| f.code).collect();
        assert_eq!(
            codes,
            vec![
                ProviderPreconditionCode::BaselineNotMet,
                ProviderPreconditionCode::RootfulOrAmbiguousEngine,
                ProviderPreconditionCode::SeccompUnavailable,
                ProviderPreconditionCode::CpuQuotaUnavailable,
            ]
        );
    }
}
