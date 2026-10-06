//! Durable operator-local implementation telemetry (F11, issue #23).
//!
//! Every AI/harness invocation is one version-tagged JSON line under
//! `$KOOLADE_HOME/projects/<project>/telemetry/` — operator-private,
//! append-oriented, and never committed to the project repository
//! (F11 spec R2-R4, AC3-AC4; store location and JSONL form settled by
//! CLR-012). Raw invocation records are the source of truth; workspace and
//! rollup figures are derived from them (R3).
//!
//! * `record.rs` — `InvocationRecord` line format (schema-version tagged)
//! * `store.rs` — concurrency-safe append store, corrupt-line quarantine

pub mod record;
pub mod report;
pub mod store;

pub use record::{InvocationRecord, SCHEMA_VERSION};
pub use report::{ImplementationMetrics, MetricBreakdown};
pub use store::{
    append, invocations_path, load, parse_record, quarantine_path, sweep_corrupt_lines,
    telemetry_dir,
};
