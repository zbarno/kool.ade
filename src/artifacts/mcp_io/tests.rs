use super::*;
use std::fs;
use std::path::PathBuf;

fn fresh_dir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_mcp_{label}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[path = "tests/probes.rs"]
mod probes;

/// Six-row rule table, incl. the normalization prohibition: a COSMETIC
/// diff (`{"a":1}` vs `{"a": 1}`) still rewrites.
#[test]
fn classify_matches_the_four_way_rule_table() {
    use McpSaveOp::*;
    assert_eq!(classify(false, None, ""), Unchanged);
    assert_eq!(classify(false, None, "{}"), Write);
    assert_eq!(classify(true, Some("{}"), "{}"), Unchanged);
    assert_eq!(classify(true, Some("{}"), "  "), Clear);
    assert_eq!(classify(true, Some("{\"a\":1}"), "{\"a\": 1}"), Write);
    assert_eq!(classify(true, Some("x"), "xy"), Write);
}

/// Runtime leg 1: well-formed save lands byte-exact in the operator-local
/// config file, carries NO git SHA (the file is never committed anywhere),
/// and leaves no temp-file residue (NFR-2).
#[test]
fn well_formed_save_writes_verified_bytes_locally() {
    let dir = fresh_dir("save");
    let buffer = "{\"mcpServers\":{\"x\":{\"command\":\"true\"}}}";
    let rec = apply_save(&dir, buffer).expect("save must succeed");
    assert_eq!(rec.op, McpSaveOp::Write);
    assert!(rec.malformed.is_none(), "valid JSON must not be flagged");
    assert!(rec.short_sha.is_none(), "local saves never commit to git");

    let path = dir.join(MCP_FILE_NAME);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        buffer,
        "bytes equal the buffer"
    );
    let tmps: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flat_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".koolade.tmp"))
        .collect();
    assert!(
        tmps.is_empty(),
        "atomic write leaves no temp residue: {tmps:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Credentials must never enter a repository: the config file is not
/// tracked by any git repository rooted at (or above) the config dir.
#[test]
fn save_stays_out_of_every_git_repository() {
    let dir = fresh_dir("norepo");
    let buffer =
        "{\"mcpServers\":{\"secret\":{\"command\":\"run\",\"env\":{\"TOKEN\":\"hunter2\"}}}";
    apply_save(&dir, buffer).expect("save must succeed");
    let path = dir.join(MCP_FILE_NAME);
    assert_eq!(fs::read_to_string(&path).unwrap(), buffer);
    let out = std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", MCP_FILE_NAME])
        .current_dir(&dir)
        .output()
        .expect("git spawn for negative tracking check");
    assert!(
        !out.status.success(),
        "mcp.json must not be tracked by a git repository"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Runtime leg 2: a blank save on an existing file removes it; a repeat
/// blank save settles to an Unchanged no-op (purge stability).
#[test]
fn blank_save_removes_file_and_repeat_blank_is_no_op() {
    let dir = fresh_dir("clear");
    let seed = "{\"mcpServers\":{\"x\":{\"command\":\"true\"}}}";
    assert_eq!(apply_save(&dir, seed).unwrap().op, McpSaveOp::Write);

    let rec = apply_save(&dir, "   ").expect("blank save must succeed");
    assert_eq!(rec.op, McpSaveOp::Clear);
    assert!(!dir.join(MCP_FILE_NAME).is_file(), "file must be gone");

    let again = apply_save(&dir, "").expect("repeat blank must settle");
    assert_eq!(again.op, McpSaveOp::Unchanged);

    let _ = fs::remove_dir_all(&dir);
}

/// Runtime leg 4: an unedited (byte-identical) second Save is a TRUE
/// no-op — file mtime+bytes untouched and the receipt names no checkpoint.
#[test]
fn unchanged_save_is_zero_file_churn() {
    let dir = fresh_dir("unchanged");
    let buffer = "{\"stable\":true,\"pad\":\"xxxxxxxxxxxxxx\"}";
    apply_save(&dir, buffer).expect("seed save");

    let path = dir.join(MCP_FILE_NAME);
    let bytes_before = fs::read(&path).unwrap();
    let mtime_before = fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);

    let rec = apply_save(&dir, buffer).expect("second save");
    assert_eq!(rec.op, McpSaveOp::Unchanged);
    assert_eq!(rec.short_sha, None, "no checkpoint for an unchanged save");

    let mtime_after = fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);
    assert_eq!(mtime_before, mtime_after, "no write occurred (mtime)");
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes_before,
        "no write occurred (bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// D-16 end-to-end: a MALFORMED fragment is saved verbatim and the
/// receipt flags the probe error; the save is never blocked.
#[test]
fn malformed_fragment_saves_verbatim_with_receipt_flag() {
    let dir = fresh_dir("malformed");
    let fragment = "{\"broken\":";

    let rec = apply_save(&dir, fragment).expect("malformed input must NOT block the save");
    assert_eq!(rec.op, McpSaveOp::Write);
    let err = rec.malformed.expect("malformed flag must be set");
    assert!(!err.trim().is_empty(), "flag carries the parse error text");

    assert_eq!(
        fs::read_to_string(dir.join(MCP_FILE_NAME)).unwrap(),
        fragment,
        "file holds the fragment VERBATIM"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Saves create the config directory when it does not exist yet, so the
/// first run never fails for want of scaffolding.
#[test]
fn save_creates_missing_local_config_directory() {
    let base = fresh_dir("nested");
    let dir = base.join("workspace/local");
    apply_save(&dir, "{}").expect("save must scaffold the directory");
    assert!(dir.join(MCP_FILE_NAME).is_file());
    let _ = fs::remove_dir_all(base);
}
