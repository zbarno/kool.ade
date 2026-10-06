//! Concurrency-safe append store for invocation records.
//!
//! Layout, operator-local and OUTSIDE the git repository (F11 spec R2,
//! AC3-AC4, settled by CLR-012):
//!
//! ```text
//! $KOOLADE_HOME/projects/<slug>/telemetry/
//!   invocations.jsonl   one record per line, appended with O_APPEND
//!   quarantine.jsonl    best-effort sink for lines this reader cannot parse
//! ```
//!
//! Writes open the file with `O_APPEND` and emit each record as a single
//! `write_all` of one complete line, so concurrent appends from several
//! tasks against the same repository keep their records intact (R3, AC8).
//! Reads tolerate corrupted or torn lines: they are skipped and counted, and
//! [`sweep_corrupt_lines`] can move them into the quarantine file — earlier
//! records always load. Creating or updating this store never touches the
//! repository worktree and is never Git-committed.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use super::record::{InvocationRecord, SCHEMA_VERSION};

const LOG_NAME: &str = "invocations.jsonl";
const QUARANTINE_NAME: &str = "quarantine.jsonl";

/// Per-project telemetry directory: `<state_root>/projects/<slug>/telemetry`.
pub fn telemetry_dir(slug: &str) -> PathBuf {
    crate::persistence::project_dir(slug).join("telemetry")
}

/// The append-only invocation log for one project slug.
pub fn invocations_path(slug: &str) -> PathBuf {
    telemetry_dir(slug).join(LOG_NAME)
}

/// Best-effort diagnostic sink for undecodable lines.
pub fn quarantine_path(slug: &str) -> PathBuf {
    telemetry_dir(slug).join(QUARANTINE_NAME)
}

/// Decode one raw log line.
///
/// `None` for lines that are not JSON, lack a recognisable `schemaVersion`,
/// or carry a schema version this build does not understand — per the
/// CLR-012 rule that readers skip unknown versions instead of bricking.
pub fn parse_record(line: &str) -> Option<InvocationRecord> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let version = value.get("schemaVersion")?.as_u64()?;
    if version != u64::from(SCHEMA_VERSION) {
        return None;
    }
    serde_json::from_value(value).ok()
}

/// Append one record as a single newline-terminated line (O_APPEND).
///
/// Guarantees the repo worktree is untouched: the target lives under the
/// operator-local state root, never under the connected repository.
pub fn append(slug: &str, record: &InvocationRecord) -> std::io::Result<()> {
    let path = invocations_path(slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(line.as_bytes())
}

/// Load all decodable records in file order.
///
/// Returns `(records, skipped)`: undecodable lines are counted, never fatal
/// (a torn trailing write must not brick history). The log itself is not
/// rewritten here; call [`sweep_corrupt_lines`] to park them separately.
pub fn load(slug: &str) -> (Vec<InvocationRecord>, usize) {
    let bytes = match fs::read(invocations_path(slug)) {
        Ok(bytes) => bytes,
        Err(_) => return (Vec::new(), 0),
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut records = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_record(line) {
            Some(record) => records.push(record),
            None => skipped += 1,
        }
    }
    (records, skipped)
}

/// Park undecodable lines in the quarantine file.
///
/// Best-effort diagnostic aid: the live log is left untouched so an in-flight
/// applier can keep writing; calling this repeatedly merely repeats the
/// parking. Returns the number of lines parked.
pub fn sweep_corrupt_lines(slug: &str) -> usize {
    let bytes = match fs::read(invocations_path(slug)) {
        Ok(bytes) => bytes,
        Err(_) => return 0,
    };
    let text = String::from_utf8_lossy(&bytes);
    let bad: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && parse_record(line).is_none())
        .collect();
    if bad.is_empty() {
        return 0;
    }
    let _ = quarantine_lines(slug, &bad);
    bad.len()
}

fn quarantine_lines(slug: &str, lines: &[&str]) -> std::io::Result<()> {
    let path = quarantine_path(slug);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::thread;

    const SLUG: &str = "tel-proj";

    // KOOLADE_HOME is process-global: whoever flips it holds this for the
    // WHOLE test body (same discipline as chat_store).
    static ENV_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn use_tmp_home(tag: &str) -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
        let lock = ENV_HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let home = env::temp_dir().join(format!("koolade_tel_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        // SAFETY: guarded by ENV_HOME_LOCK; no other test mutates or depends
        // on KOOLADE_HOME while the guard is held.
        unsafe { env::set_var("KOOLADE_HOME", &home) };
        (home, lock)
    }

    fn rec(task: &str, feature: &str) -> InvocationRecord {
        let mut r = InvocationRecord::new("tel-project", "pi");
        r.feature = Some(feature.to_owned());
        r.task = Some(task.to_owned());
        r.input_tokens = Some(100);
        r.total_tokens = Some(130);
        r.outcome = "completed".into();
        r
    }

    fn inject_raw(slug: &str, fragment: &str) {
        let mut f = OpenOptions::new()
            .append(true)
            .open(invocations_path(slug))
            .unwrap();
        f.write_all(fragment.as_bytes()).unwrap();
    }

    #[test]
    fn append_yields_one_parseable_line_carrying_full_attribution() {
        let (_home, _env) = use_tmp_home("rt");
        let mut full = rec("task-full", "F11");
        full.session = Some("sess-1".into());
        full.repository = Some("zbarno/kool.ade".into());
        full.batch = Some("F11-02".into());
        full.phase = Some("implementation".into());
        full.provider = Some("anthropic".into());
        full.api = Some("messages".into());
        full.model = Some("claude-sonnet-4-5".into());
        full.requested_model = Some("claude-sonnet-4-5".into());
        full.thinking_level = Some("medium".into());
        full.model_calls = Some(17);
        full.output_tokens = Some(29_841);
        full.cache_read_tokens = Some(691_440);
        full.cache_write_tokens = Some(38_212);
        full.reasoning_tokens = Some(18_104);
        full.started_at = chrono::DateTime::from_timestamp_millis(1_760_000_000_000);
        full.ended_at = chrono::DateTime::from_timestamp_millis(1_760_000_711_000);
        full.duration_millis = Some(711_000);
        full.estimated_cost_usd_cents = Some(184);
        full.price_table_version = Some("price_table_v1_2025Q3".into());

        append(SLUG, &full).unwrap();
        let text = fs::read_to_string(invocations_path(SLUG)).unwrap();
        assert_eq!(text.lines().count(), 1, "exactly one line per record");
        assert!(text.ends_with('\n'));
        let (records, skipped) = load(SLUG);
        assert_eq!((records, skipped), (vec![full], 0));
    }

    #[test]
    fn concurrent_appends_from_separate_tasks_keep_every_record_intact() {
        let (_home, _env) = use_tmp_home("conc");
        let handles: Vec<_> = (0..4)
            .map(|worker| {
                thread::spawn(move || {
                    for step in 0..25 {
                        let r = rec(&format!("task-{worker}-{step}"), &format!("F11-W{worker}"));
                        append(SLUG, &r).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let (records, skipped) = load(SLUG);
        assert_eq!(records.len(), 100, "no record lost or interleaved");
        assert_eq!(skipped, 0, "no line damaged by concurrent appends");
        let tasks: std::collections::HashSet<_> =
            records.iter().filter_map(|r| r.task.clone()).collect();
        assert_eq!(tasks.len(), 100, "every task identity is distinct");
        let features: std::collections::HashSet<_> =
            records.iter().filter_map(|r| r.feature.clone()).collect();
        assert_eq!(
            features.len(),
            4,
            "concurrent tasks stay attributable apart"
        );
        assert!(records.iter().all(|r| r.project == "tel-project"));
    }

    #[test]
    fn corrupt_lines_skip_without_failing_earlier_reads_and_get_quarantined() {
        let (_home, _env) = use_tmp_home("corrupt");
        append(SLUG, &rec("task-before", "F11")).unwrap();
        inject_raw(SLUG, "THIS LINE IS NOT JSON\n");
        append(SLUG, &rec("task-after", "F11")).unwrap();
        inject_raw(SLUG, "{\"schemaVersion\":1,\"project\");"); // torn trailing write

        let (records, skipped) = load(SLUG);
        assert_eq!(skipped, 2);
        assert_eq!(
            records
                .iter()
                .filter_map(|r| r.task.clone())
                .collect::<Vec<_>>(),
            vec!["task-before".to_owned(), "task-after".to_owned()],
            "prior and later records still load, in order"
        );

        assert_eq!(sweep_corrupt_lines(SLUG), 2);
        let quarantined = fs::read_to_string(quarantine_path(SLUG)).unwrap();
        assert!(quarantined.contains("THIS LINE IS NOT JSON"));
        assert!(quarantined.contains("{\"schemaVersion\":1,\"project\");"));
        let (again, _) = load(SLUG);
        assert_eq!(again.len(), 2, "reading again is stable");
    }

    #[test]
    fn foreign_schema_versions_are_skipped_not_fatal() {
        let (_home, _env) = use_tmp_home("ver");
        append(SLUG, &rec("task-now", "F11")).unwrap();
        inject_raw(
            SLUG,
            "{\"schemaVersion\":99,\"project\":\"tel-project\",\"harness\":\"pi\",\"outcome\":\"x\"}\n",
        );
        let (records, skipped) = load(SLUG);
        assert_eq!((records.len(), skipped), (1, 1));
        assert_eq!(records[0].task.as_deref(), Some("task-now"));
    }

    #[test]
    fn writes_land_under_the_state_root_never_the_repo_worktree() {
        let (home, _env) = use_tmp_home("separate");
        append(SLUG, &rec("task-local", "F11")).unwrap();
        let home = home.canonicalize().unwrap();
        let dir = telemetry_dir(SLUG).canonicalize().unwrap();
        assert_eq!(
            dir,
            home.join("projects").join(SLUG).join("telemetry"),
            "telemetry stays rooted at the operator-local state root"
        );
        // The worktree-negation only makes sense when the state root itself
        // sits outside the repository (true on every real host).
        if !home.starts_with(env!("CARGO_MANIFEST_DIR")) {
            assert!(
                !dir.starts_with(env!("CARGO_MANIFEST_DIR")),
                "telemetry must not live inside the repository worktree"
            );
        }
        let entries = fs::read_dir(home.join("projects").join(SLUG))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["telemetry"]);
    }
}
