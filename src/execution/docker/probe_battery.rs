//! Probe battery scaffold (issue #591, phase 2).
//!
//! This module turns the design document's 16-row threat enumeration
//! (`docs/provider-execution-containment-v1.md`, §Threat enumeration) into a
//! typed, data-driven catalog so that "what must be proven" is enforceable
//! code, not prose:
//!
//! - Each threat vector carries its boundary tags (M/E/N/P/D/T/C/S), the
//!   control grade the design assigns, and BOTH sides of the probe: what the
//!   probe does (`probe`) and the observable contained/breached signal
//!   (`oracle`).
//! - Execution permissions are decided BY CONSTRUCTION. Three classes exist:
//!   hermetic-safe (pure logic/state checks), contained-only (must run inside
//!   a rootless-contained daemon environment), and manual-evidence-only
//!   (kernel-CVE-class and residual-disclosure vectors that must NEVER run
//!   automatically; their evidence is recorded as human-attested claims).
//! - No probe EXECUTION is wired in this slice: the shipped runner is
//!   structural and reports every vector as pending implementation. That is
//!   the honest state — gates, taxonomy, and oracles are in place; execution
//!   lands with the rootless-Docker environment slice, which is why this
//!   crate compiles and tests green on hosts with no daemon.
//!
//! Integrity guards: catalog completeness (16 vectors, unique contiguous
//! numbering), full coverage of the eight boundary letters, and a grade
//! SHADOW-TABLE check. Scope of that check, stated plainly: it detects
//! unilateral code-side edits (catalog or shadow table changed alone) but
//! NOT a coordinated edit of doc + both tables; the doc leg rests on review
//! discipline, and a cross-language parser anchor is a later-slice item.

/// Containment boundary a threat vector crosses, mirroring the design
/// document's letter taxonomy (M-mounts, E-env/secrets, N-network,
/// P-process, D-daemon, T-task, C-cancellation, S-secrecy-of-app-authority).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryTag {
    Mounts,
    EnvOrSecrets,
    Network,
    Process,
    Daemon,
    Task,
    Cancellation,
    AppAuthoritySecrecy,
}

impl BoundaryTag {
    /// Stable letter used by the design document; tests anchor on it.
    pub fn letter(self) -> &'static str {
        match self {
            BoundaryTag::Mounts => "M",
            BoundaryTag::EnvOrSecrets => "E",
            BoundaryTag::Network => "N",
            BoundaryTag::Process => "P",
            BoundaryTag::Daemon => "D",
            BoundaryTag::Task => "T",
            BoundaryTag::Cancellation => "C",
            BoundaryTag::AppAuthoritySecrecy => "S",
        }
    }
}

/// The confidence grade the design document assigns to each vector's control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlGrade {
    Enforced,
    EnforcedResidualRisk,
    PrototypeRequired,
    ResidualDisclosed,
}

/// How severe the vector is if it succeeds pre-containment. Used for report
/// prioritization only; grading lives with the control side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorSeverity {
    Medium,
    High,
    Critical,
}

/// Whether a vector may ever execute automatically, and in which contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionPermission {
    /// Pure logic/state checks; safe everywhere, including CI units.
    HermeticSafe,
    /// Must run with a verified rootless-contained daemon target.
    ContainedOnly,
    /// Kernel-CVE-class or disclosed-residual vectors: never automated;
    /// evidence is recorded as human-attested claims instead.
    ManualEvidenceOnly,
}

/// The probe environment the executing host declares about ITSELF. Gates
/// treat unrecognized environments conservatively by construction (nothing
/// beyond hermetic may run).
// Default = the weakest context (InMemoryHermetic), so default-constructed
// runners can never claim more context than they prove.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProbeEnvironment {
    /// Unit-test / in-process context. Only hermetic probes qualify.
    #[default]
    InMemoryHermetic,
    /// Verified rootless Docker daemon with the session boundaries in force.
    ContainedRootlessDaemon,
}

/// One row of the threat matrix. Numbers are STABLE identifiers anchored to
/// the design document table (1..=16); renumbering is a breaking change to
/// evidence references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreatVector {
    pub number: u8,
    pub title: &'static str,
    pub boundaries: &'static [BoundaryTag],
    pub grade: ControlGrade,
    pub severity: VectorSeverity,
    /// How the vector lands pre-fix (attack-side description).
    pub attack: &'static str,
    /// The contained-behavior expectation (defense-side description).
    pub contained_behavior: &'static str,
    /// What the automated/manual probe performs.
    pub probe: &'static str,
    /// Observable signal separating contained from breached.
    pub oracle: &'static str,
    pub execution: ExecutionPermission,
}

pub mod catalog;

/// The complete threat matrix (defined in `catalog.rs` so this module
/// stays lean); numbers are stable evidence identifiers.
pub use catalog::THREAT_VECTORS;

/// Disposition of a single vector under a runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeDisposition {
    /// The probe ran and observed the contained behavior.
    ConfirmedContained { number: u8 },
    /// THE ORACLE OBSERVED BREACHED BEHAVIOR: this is a red-alert condition.
    Breached { number: u8, evidence: String },
    /// The vector's execution class forbids automated execution here.
    ManualEvidenceRequired { number: u8 },
    /// The vector is authorized for this environment but execution is not
    /// wired yet (the honest state of this scaffold).
    PendingImplementation { number: u8 },
}

/// Gate: may `vector` execute automatically in `environment`? Fail-closed:
/// unknown combinations refuse.
pub fn may_execute(vector: &ThreatVector, environment: ProbeEnvironment) -> bool {
    match vector.execution {
        ExecutionPermission::HermeticSafe => true,
        ExecutionPermission::ContainedOnly => {
            environment == ProbeEnvironment::ContainedRootlessDaemon
        }
        ExecutionPermission::ManualEvidenceOnly => false,
    }
}

/// Aggregated, deterministic battery report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatterySummary {
    pub confirmed_contained: Vec<u8>,
    pub breached: Vec<(u8, String)>,
    pub manual_evidence_required: Vec<u8>,
    pub pending_implementation: Vec<u8>,
}

impl BatterySummary {
    /// Red-alert predicate: ANY breached vector fails the battery outright.
    pub fn is_clear(&self) -> bool {
        self.breached.is_empty()
    }

    /// Aggregate raw dispositions into a deterministically sorted summary.
    pub fn from_dispositions(dispositions: &[ProbeDisposition]) -> BatterySummary {
        let mut out = BatterySummary::default();
        for d in dispositions {
            match d {
                ProbeDisposition::ConfirmedContained { number } => {
                    out.confirmed_contained.push(*number)
                }
                ProbeDisposition::Breached { number, evidence } => {
                    out.breached.push((*number, evidence.clone()))
                }
                ProbeDisposition::ManualEvidenceRequired { number } => {
                    out.manual_evidence_required.push(*number)
                }
                ProbeDisposition::PendingImplementation { number } => {
                    out.pending_implementation.push(*number)
                }
            }
        }
        out.sort_deterministic();
        out
    }

    fn sort_deterministic(&mut self) {
        self.confirmed_contained.sort_unstable();
        self.breached
            .sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        self.manual_evidence_required.sort_unstable();
        self.pending_implementation.sort_unstable();
    }
}

/// Port for executing the battery against a real target. Kept narrow so the
/// execution slice (rootless-Docker aware) can supply the implementation
/// without touching the catalog or the gates. BINDING REQUIREMENT FOR THAT
/// SLICE: the environment a runner attests must be coupled to VERIFIED
/// evidence (e.g., the #714 rootless preflight result), and dispositions
/// must carry tamper-evident probe output — today the honor system is safe
/// only because the sole shipped runner physically cannot execute.
pub trait ProbeRunner {
    /// The environment THIS runner claims to operate in; the catalog trusts
    /// the gate's word about the runner only when the runner attests it.
    fn environment(&self) -> ProbeEnvironment;
    /// Run ONE vector. Implementations must respect `may_execute`; a runner
    /// that executes a forbidden vector defeats the whole model.
    fn run_vector(&mut self, vector: &ThreatVector) -> ProbeDisposition;
}

/// Scaffold runner: honest non-executor. Reports every vector according to
/// the gates without performing ANY side effects. This is what ships until
/// the rootless-Docker execution slice lands, which makes the merged state
/// provably inert (no accidental probe detonation).
#[derive(Debug, Clone, Copy, Default)]
pub struct StructuralProbeRunner {
    pub environment: ProbeEnvironment,
}

impl ProbeRunner for StructuralProbeRunner {
    fn environment(&self) -> ProbeEnvironment {
        self.environment
    }

    fn run_vector(&mut self, vector: &ThreatVector) -> ProbeDisposition {
        // Gate-conformant by construction: manual-evidence vectors are
        // labeled for humans; EVERYTHING else (including contained-only
        // vectors in a hermetic context, where may_execute says no) is
        // reported pending. A non-executor must never confirm.
        match vector.execution {
            ExecutionPermission::ManualEvidenceOnly => ProbeDisposition::ManualEvidenceRequired {
                number: vector.number,
            },
            _ => ProbeDisposition::PendingImplementation {
                number: vector.number,
            },
        }
    }
}

/// Run the full battery with one runner; panics if the runner reports an
/// environment inconsistent with its dispositions (defense against lying
/// runners slipping past the gates).
pub fn run_battery(runner: &mut dyn ProbeRunner, environment: ProbeEnvironment) -> BatterySummary {
    assert_eq!(
        runner.environment(),
        environment,
        "runner environment attestation mismatch: catalog runs gate on the DECLARED environment"
    );
    let dispositions: Vec<ProbeDisposition> = THREAT_VECTORS
        .iter()
        .map(|v| runner.run_vector(v))
        .collect();
    BatterySummary::from_dispositions(&dispositions)
}

/// Catalog integrity violations. An EMPTY list means the matrix is healthy:
/// 16 unique contiguous numbers, all fields populated, all eight boundary
/// letters represented, and every vector's grade/severity well-formed.
pub fn catalog_integrity_violations() -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    if THREAT_VECTORS.len() != 16 {
        problems.push(format!(
            "expected exactly 16 threat vectors (matching the design table), found {}",
            THREAT_VECTORS.len()
        ));
    }
    let mut seen_numbers = std::collections::BTreeSet::new();
    let mut letters_seen: Vec<BoundaryTag> = Vec::new();
    for v in THREAT_VECTORS {
        if !(1..=16).contains(&v.number) {
            problems.push(format!("vector number {:?} outside 1..=16", v.number));
        }
        if !seen_numbers.insert(v.number) {
            problems.push(format!("duplicate vector number {}", v.number));
        }
        if v.title.trim().is_empty() || v.attack.trim().is_empty() {
            problems.push(format!("vector {}: blank title/attack", v.number));
        }
        if v.contained_behavior.trim().is_empty()
            || v.probe.trim().is_empty()
            || v.oracle.trim().is_empty()
        {
            problems.push(format!("vector {}: blank behavior/probe/oracle", v.number));
        }
        if v.boundaries.is_empty() {
            problems.push(format!("vector {}: no boundary tags", v.number));
        }
        letters_seen.extend(v.boundaries.iter().copied());
    }
    if seen_numbers.len() != 16 {
        problems.push(format!(
            "expected 16 unique vector numbers, found {}",
            seen_numbers.len()
        ));
    }
    let all_letters: Vec<BoundaryTag> = vec![
        BoundaryTag::Mounts,
        BoundaryTag::EnvOrSecrets,
        BoundaryTag::Network,
        BoundaryTag::Process,
        BoundaryTag::Daemon,
        BoundaryTag::Task,
        BoundaryTag::Cancellation,
        BoundaryTag::AppAuthoritySecrecy,
    ];
    for letter in all_letters {
        if !letters_seen.contains(&letter) {
            problems.push(format!(
                "boundary letter '{}' (taxonomy) has no vector; expand the matrix or fix the taxonomy",
                letter.letter()
            ));
        }
    }
    // Deterministic dedupe, keep order of first appearance.
    let mut seen = std::collections::BTreeSet::new();
    problems.retain(|p| seen.insert(p.clone()));
    problems
}

/// The graded-threat matrix mirrors `docs/provider-execution-containment-v1.md`
/// §Threat enumeration; these tables are the executable half of that doc.
#[cfg(test)]
mod tests {
    use super::*;

    /// Grade shadow table replicating the design document's grade column.
    /// Detects unilateral code-side edits only (NOT coordinated
    /// doc+table edits — the doc leg is review discipline until a
    /// cross-language anchor exists); see the module header. If the
    /// document's grade for a vector changes, the catalog constant
    /// (catalog.rs) AND this table must change together — deliberate friction.
    const DOC_GRADES_BY_NUMBER: &[(u8, ControlGrade)] = &[
        (1, ControlGrade::Enforced),
        (2, ControlGrade::Enforced),
        (3, ControlGrade::Enforced),
        (4, ControlGrade::EnforcedResidualRisk),
        (5, ControlGrade::Enforced),
        (6, ControlGrade::Enforced),
        (7, ControlGrade::EnforcedResidualRisk),
        (8, ControlGrade::Enforced),
        (9, ControlGrade::Enforced),
        (10, ControlGrade::ResidualDisclosed),
        (11, ControlGrade::Enforced),
        (12, ControlGrade::Enforced),
        (13, ControlGrade::Enforced),
        (14, ControlGrade::Enforced),
        (15, ControlGrade::Enforced),
        (16, ControlGrade::Enforced),
    ];

    use std::panic::AssertUnwindSafe;

    #[test]
    fn catalog_is_complete_wellformed_and_stable() {
        assert!(
            catalog_integrity_violations().is_empty(),
            "violations: {:?}",
            catalog_integrity_violations()
        );
        let nums: Vec<u8> = THREAT_VECTORS.iter().map(|v| v.number).collect();
        assert_eq!(
            nums,
            (1..=16).collect::<Vec<_>>(),
            "numbers must ascend 1..=16"
        );
    }

    #[test]
    fn doc_grades_match_the_design_table() {
        for v in THREAT_VECTORS {
            let expected = DOC_GRADES_BY_NUMBER
                .iter()
                .find(|(n, _)| *n == v.number)
                .map(|(_, g)| *g)
                .expect("doc-grade row present");
            assert_eq!(v.grade, expected, "grade drift on vector {}", v.number);
        }
    }

    #[test]
    fn boundary_taxonomy_fully_covered() {
        let used: std::collections::BTreeSet<&str> = THREAT_VECTORS
            .iter()
            .flat_map(|v| v.boundaries.iter().copied().map(BoundaryTag::letter))
            .collect();
        assert_eq!(
            used,
            ["M", "E", "N", "P", "D", "T", "C", "S"]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        );
    }

    #[test]
    fn gate_forbids_dangerous_classes_outside_safe_contexts() {
        // Manual-evidence-only vectors (7, 10, 13, 16) NEVER execute
        // automatically, in any environment.
        for v in THREAT_VECTORS {
            if v.execution == ExecutionPermission::ManualEvidenceOnly {
                assert!(
                    !may_execute(v, ProbeEnvironment::ContainedRootlessDaemon)
                        && !may_execute(v, ProbeEnvironment::InMemoryHermetic),
                    "manual-evidence vector {} may never auto-execute",
                    v.number
                );
            }
        }
        // Contained-only vectors refuse the hermetic context.
        for v in THREAT_VECTORS {
            if v.execution == ExecutionPermission::ContainedOnly {
                assert!(!may_execute(v, ProbeEnvironment::InMemoryHermetic));
                assert!(may_execute(v, ProbeEnvironment::ContainedRootlessDaemon));
            }
            if v.execution == ExecutionPermission::HermeticSafe {
                assert!(may_execute(v, ProbeEnvironment::InMemoryHermetic));
            }
        }
        // Exactly two hermetic vectors exist: 12 and 15.
        let hermetic: Vec<u8> = THREAT_VECTORS
            .iter()
            .filter(|v| v.execution == ExecutionPermission::HermeticSafe)
            .map(|v| v.number)
            .collect();
        assert_eq!(hermetic, vec![12, 15]);
    }

    #[test]
    fn structural_runner_is_provably_inert() {
        // In the strongest context, NOTHING confirms or breaches: the
        // scaffold can only report pending/manual — a false ConfirmedContained
        // would be worse than useless (fake clearance).
        let mut runner = StructuralProbeRunner {
            environment: ProbeEnvironment::ContainedRootlessDaemon,
        };
        let summary = run_battery(&mut runner, ProbeEnvironment::ContainedRootlessDaemon);
        assert!(
            summary.confirmed_contained.is_empty(),
            "no confirmations from a non-executing runner"
        );
        assert!(
            summary.breached.is_empty(),
            "no breach claims from a non-executing runner"
        );
        let manual_count = THREAT_VECTORS
            .iter()
            .filter(|v| v.execution == ExecutionPermission::ManualEvidenceOnly)
            .count();
        assert_eq!(
            summary.manual_evidence_required.len(),
            manual_count,
            "manual vectors are labeled, not skipped"
        );
        assert_eq!(
            summary.pending_implementation.len(),
            16 - manual_count,
            "everything else is honestly pending"
        );
    }

    #[test]
    fn summary_aggregation_is_deterministic_and_red_alert_on_any_breach() {
        let ds = vec![
            ProbeDisposition::PendingImplementation { number: 3 },
            ProbeDisposition::ConfirmedContained { number: 12 },
            ProbeDisposition::Breached {
                number: 5,
                evidence: "socket reachable".into(),
            },
            ProbeDisposition::ManualEvidenceRequired { number: 10 },
        ];
        let s = BatterySummary::from_dispositions(&ds);
        assert!(!s.is_clear(), "any breach trips the red alert");
        assert_eq!(s.breached.first().map(|(n, _)| *n), Some(5));
        assert_eq!(s.confirmed_contained, vec![12]);
        assert_eq!(s.manual_evidence_required, vec![10]);
        assert_eq!(s.pending_implementation, vec![3]);
        // Re-aggregating yields an identical structure (sorted determinism).
        assert_eq!(s, BatterySummary::from_dispositions(&ds));
    }

    #[test]
    fn runner_environment_attestation_is_enforced() {
        // A runner attesting one environment while the catalog declares
        // another must be caught — the gate decides on the DECLAREd context.
        let mut liar = StructuralProbeRunner {
            environment: ProbeEnvironment::ContainedRootlessDaemon,
        };
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_battery(&mut liar, ProbeEnvironment::InMemoryHermetic)
        }));
        assert!(
            result.is_err(),
            "attestation mismatch must abort, not proceed"
        );
    }

    #[test]
    fn execution_class_distribution_matches_design_intent() {
        let manual: Vec<u8> = THREAT_VECTORS
            .iter()
            .filter(|v| v.execution == ExecutionPermission::ManualEvidenceOnly)
            .map(|v| v.number)
            .collect();
        assert_eq!(
            manual,
            vec![7, 10, 13, 16],
            "kernel-CVE-class, residual-disclosure, and Manager-gate vectors stay manual"
        );
        let contained: Vec<u8> = THREAT_VECTORS
            .iter()
            .filter(|v| v.execution == ExecutionPermission::ContainedOnly)
            .map(|v| v.number)
            .collect();
        assert_eq!(
            contained.len(),
            10,
            "ten vectors await contained-environment execution"
        );
    }
}
