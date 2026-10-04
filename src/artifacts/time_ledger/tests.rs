//! Focused tests for the git-backed time ledger: round-trips, ordering,
//! fault injection (torn final line), crash-window branches, and strict
//! rejection of malformed rows.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::format::decode_line;
use super::*;

static SCRATCH_SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "koolade-ledger-{label}-{}-{}",
        std::process::id(),
        SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Mirror the parent-directory creation [`append_row`] gets from atomic writes.
fn ensure_slot(root: &Path) {
    let path = ledger_path(root);
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
}

fn sample(i: usize, status: EndStatus) -> IntervalRow {
    let needs_end = status != EndStatus::InterruptedDiscard;
    IntervalRow {
        repo_id: "root".into(),
        workspace_id: format!("ws-{i}"),
        session_id: format!("sess-{i}"),
        worker_pid: Some(1000 + i as u64),
        start_epoch_s: 1_700_000_000 + i as u64,
        end_epoch_s: needs_end.then_some(1_700_000_000 + 3600 + i as u64),
        item_uid: format!("task-uid-{i:04}"),
        feature_ref: i.is_multiple_of(2).then_some("F7".into()),
        end_status: status,
    }
}

fn valid_combinations() -> Vec<IntervalRow> {
    let statuses = [
        EndStatus::Ended,
        EndStatus::InterruptedDiscard,
        EndStatus::InterruptedCount,
    ];
    let mut out = Vec::new();
    for (s, status) in statuses.iter().enumerate() {
        for pids in [true, false] {
            for feats in [true, false] {
                let mut row = sample(s, *status);
                row.worker_pid = pids.then_some(row.worker_pid.unwrap_or(7));
                row.feature_ref = feats.then_some(row.feature_ref.take().unwrap_or("F7".into()));
                if *status == EndStatus::InterruptedDiscard {
                    row.end_epoch_s = pids.then_some(1_700_000_000 + 90);
                }
                out.push(row);
            }
        }
    }
    out
}

#[test]
fn round_trip_each_variant_keeps_fields_and_canonical_bytes() {
    for row in valid_combinations() {
        let line = serialize_line(&row);
        assert!(line.ends_with('\n'));
        let decoded = decode_line(line.trim_end_matches('\n')).unwrap();
        assert_eq!(decoded, row, "round trip drifted for {line:?}");
        assert_eq!(serialize_line(&decoded), line, "bytes not canonical");
    }
}

#[test]
fn five_sequential_appends_reload_byte_identical_in_stable_order() {
    let root = scratch("five");
    let rows: Vec<IntervalRow> = (0..5).map(|i| sample(i, EndStatus::Ended)).collect();
    for row in &rows {
        append_row(&root, row).unwrap();
    }
    let mut expected = String::from(HEADER_LINE);
    expected.push('\n');
    for row in &rows {
        expected.push_str(&serialize_line(row));
    }
    let raw = std::fs::read_to_string(ledger_path(&root)).unwrap();
    assert_eq!(raw, expected, "on-disk bytes drifted from canonical");
    let reloaded = load(&root).unwrap();
    assert_eq!(reloaded, rows, "parsed rows not byte-identical to appended");
}

#[test]
fn append_after_missing_file_creates_directories_and_header() {
    let root = scratch("fresh");
    assert!(!ledger_path(&root).exists());
    append_row(&root, &sample(0, EndStatus::Ended)).unwrap();
    append_row(&root, &sample(1, EndStatus::Ended)).unwrap();
    let raw = std::fs::read_to_string(ledger_path(&root)).unwrap();
    assert!(
        raw.starts_with(&format!("{HEADER_LINE}\n")),
        "header line restored"
    );
    assert_eq!(load(&root).unwrap().len(), 2);
    assert!(ledger_path(&root).parent().unwrap().is_dir());
}

#[test]
fn missing_file_loads_empty_without_creating_anything() {
    let root = scratch("absent");
    assert!(load(&root).unwrap().is_empty());
    assert!(!ledger_path(&root).exists());
}

#[test]
fn torn_final_line_salvages_one_interrupted_discard_per_cut_depth() {
    for (label, cut) in [
        ("shallow", 4_usize),
        ("deep", 12_usize),
        ("newline-only", 1_usize),
    ] {
        let root = scratch(label);
        let rows: Vec<IntervalRow> = (0..3).map(|i| sample(i, EndStatus::Ended)).collect();
        for row in &rows {
            append_row(&root, row).unwrap();
        }
        // The separately written valid file carries the intact rows only, so its
        // load is equivalent modulo the one synthetic discard.
        let clean_twin = scratch(&format!("{label}-twin"));
        for row in &rows[..2] {
            append_row(&clean_twin, row).unwrap();
        }
        let raw = std::fs::read(ledger_path(&root)).unwrap();
        let cut_point = raw.len().saturating_sub(cut);
        std::fs::write(ledger_path(&root), &raw[..cut_point]).unwrap();

        let torn = load(&root).unwrap();
        // Three appends with the last row torn: two complete rows plus one discard.
        assert_eq!(
            torn.len(),
            3,
            "{label}: expected complete rows plus one discard"
        );
        assert_eq!(&torn[..2], &rows[..2], "{label}: complete rows drifted");
        assert_eq!(
            torn[2].end_status,
            EndStatus::InterruptedDiscard,
            "{label}: torn row must classify as InterruptedDiscard"
        );
        assert_eq!(torn[2].repo_id, "root", "{label}: prefix salvage lost");
        // Separately written valid file: equivalent vec modulo the synthetic discard.
        assert_eq!(load(&clean_twin).unwrap(), torn[..2]);
    }
}

#[test]
fn cut_landing_on_line_boundary_leaves_no_salvage_row() {
    let root = scratch("boundary");
    let rows: Vec<IntervalRow> = (0..3).map(|i| sample(i, EndStatus::Ended)).collect();
    for row in &rows {
        append_row(&root, row).unwrap();
    }
    let raw = std::fs::read(ledger_path(&root)).unwrap();
    // Drop the final row outright: the file then ends exactly on its own newline.
    let after_second_row = raw[..raw.len() - 1]
        .iter()
        .rposition(|needle| *needle == b'\n')
        .unwrap()
        + 1;
    std::fs::write(ledger_path(&root), &raw[..after_second_row]).unwrap();
    let got = load(&root).unwrap();
    assert_eq!(
        got.as_slice(),
        &rows[..2],
        "terminus on a newline must not invent rows"
    );
}

#[test]
fn crash_window_yields_pre_crash_or_post_crash_never_partial() {
    let root = scratch("crash");
    let pre: Vec<IntervalRow> = (0..2).map(|i| sample(i, EndStatus::Ended)).collect();
    for row in &pre {
        append_row(&root, row).unwrap();
    }
    let file = ledger_path(&root);
    assert_eq!(load(&root).unwrap(), pre);

    // Branch A: killed between write_all and rename — the unreplaced temp twin
    // is ignored and the pre-crash file loads unchanged.
    let mut post_text = String::from(HEADER_LINE);
    post_text.push('\n');
    for row in pre.iter().chain([&sample(2, EndStatus::Ended)]) {
        post_text.push_str(&serialize_line(row));
    }
    let pending = file.with_file_name("time-ledger.log.pending-sim");
    std::fs::write(&pending, post_text).unwrap();
    assert_eq!(
        load(&root).unwrap(),
        pre,
        "pending temp must not leak into load"
    );

    // Branch B: rename completed before the kill — full snapshot loads instead.
    let post: Vec<IntervalRow> = (0..3).map(|i| sample(i, EndStatus::Ended)).collect();
    std::fs::rename(&pending, &file).unwrap();
    assert_eq!(
        load(&root).unwrap(),
        post,
        "renamed file must load complete"
    );
}

#[test]
fn append_after_tear_crystallizes_a_stable_strict_discard_row() {
    let root = scratch("solidify");
    let base: Vec<IntervalRow> = (0..2).map(|i| sample(i, EndStatus::Ended)).collect();
    for row in &base {
        append_row(&root, row).unwrap();
    }
    let file = ledger_path(&root);
    let raw = std::fs::read(&file).unwrap();
    std::fs::write(&file, &raw[..raw.len() - 5]).unwrap();

    let first = load(&root).unwrap();
    // Two appends with the last row torn: one complete row plus one discard.
    assert_eq!(first.len(), 2);
    assert_eq!(first[0], base[0]);
    assert_eq!(first[1].end_status, EndStatus::InterruptedDiscard);

    append_row(&root, &sample(9, EndStatus::Ended)).unwrap();
    let second = load(&root).unwrap();
    assert_eq!(second.len(), 3);
    assert_eq!(
        second[1], first[1],
        "discarded row must crystallize unchanged"
    );
    assert_eq!(second[2], sample(9, EndStatus::Ended));

    let content = std::fs::read_to_string(&file).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(
        decode_line(lines[2]).is_ok(),
        "crystallized discard row must re-parse strictly"
    );
    assert_eq!(
        load(&root).unwrap(),
        second,
        "reloads must be deterministic"
    );
}

#[test]
fn strict_decode_rejects_malformed_lines() {
    let good = serialize_line(&sample(0, EndStatus::Ended));
    let base = good.trim_end_matches('\n');
    let eight_columns = base
        .split_once(':')
        .map(|(_, rest)| rest.to_owned())
        .expect("colons");
    let eleven_columns = format!("{base}:extra");
    let wrong_tag = format!("L{}", &base[1..]);
    let bad_lines: Vec<(&str, String)> = vec![
        (
            "unknown end_status",
            base.replacen("ended", "finishing-touches", 1),
        ),
        ("case-sensitive token", base.replacen("ended", "ENDED", 1)),
        (
            "non-decimal epoch",
            base.replacen("1700000000", "1700O0OOOO", 1),
        ),
        (
            "signed epoch",
            base.replacen("1700000000", "+1700000000", 1),
        ),
        (
            "overflow epoch",
            base.replacen("1700000000", "99999999999999999999", 1),
        ),
        ("eight columns", eight_columns),
        ("eleven columns", eleven_columns),
        ("empty repo id", base.replacen("T:root:", "T::", 1)),
        ("control char in text", base.replacen("ws-0", "ws\t0", 1)),
        ("comma smuggled", base.replacen("ws-0", "ws,0", 1)),
        (
            "end before start",
            base.replacen("1700003600", "1699999999", 1),
        ),
        ("wrong tag", wrong_tag),
    ];
    for (label, line) in &bad_lines {
        assert!(
            decode_line(line).is_err(),
            "{label} must be rejected: {line:?}"
        );
    }
    assert_eq!(decode_line(base).unwrap(), sample(0, EndStatus::Ended));
    assert!(
        good.ends_with('\n') && decode_line(&good).is_err(),
        "a bare trailing newline is not a line"
    );
}

#[test]
fn file_level_corruption_aborts_instead_of_guessing() {
    let row = serialize_line(&sample(0, EndStatus::Ended));
    let cases: Vec<(&str, String)> = vec![
        ("bad header", format!("time-ledger:v9\n{row}")),
        ("blank-interior-line", format!("{HEADER_LINE}\n\n{row}")),
        (
            "malformed-interior-row",
            format!("{HEADER_LINE}\ngarbage\n{row}"),
        ),
        ("zero-byte-file", String::new()),
    ];
    for (label, content) in cases {
        let root = scratch(label);
        ensure_slot(&root);
        std::fs::write(ledger_path(&root), content).unwrap();
        assert!(load(&root).is_err(), "{label} must abort the load");
    }
    let root = scratch("nonutf8");
    ensure_slot(&root);
    let mut bytes = format!("{HEADER_LINE}\n").into_bytes();
    bytes.push(0xff);
    std::fs::write(ledger_path(&root), bytes).unwrap();
    assert!(load(&root).is_err(), "non-UTF-8 must abort the load");
}
