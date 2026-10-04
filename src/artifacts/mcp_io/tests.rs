use super::*;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fresh_dir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_mcp_{label}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn git_in(p: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Bootstrap-only git step: a silent fixture init failure masquerades as
/// a confusing downstream error (e.g. "not a git repository" at the
/// later `git add`). The certification gate (ticket 006) requires the
/// FIRST failure to speak at its own site with its own stderr, so an
/// environmental hit (power blip, external kill of the git child, fs
/// fault) is diagnosed where it landed. Read-side lookups keep using
/// `git_in` (status-insensitive).
fn git_mk(p: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "fixture git spawn {:?} in {} failed: {e}",
                args,
                p.display()
            )
        });
    assert!(
        out.status.success(),
        "fixture git {:?} failed in {}: exit={:?}\nstderr: {}",
        args,
        p.display(),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Hand-rolled temp repo (same pattern as `gitops::tests::mkrepo` — no
/// `tempfile` crate, NFR-7). Local identity so ambient git config can
/// never leak into assertions.
fn mkrepo(label: &str) -> PathBuf {
    let p = fresh_dir(label);
    git_mk(&p, &["init", "-q", "-b", "main"]);
    git_mk(&p, &["config", "user.name", "T"]);
    git_mk(&p, &["config", "user.email", "t@x"]);
    fs::write(p.join("a.txt"), "one").unwrap();
    git_mk(&p, &["add", "a.txt"]);
    git_mk(&p, &["commit", "-qm", "init"]);
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

/// AC leg 1 (runtime): well-formed save on a git repo → bytes equal the
/// buffer, file tracked, dedicated subject + author, 7-char SHA matches
/// real git, porcelain-clean tree, no temp-file residue (NFR-2).
#[test]
fn well_formed_save_writes_verified_bytes_and_commits_update_checkpoint() {
    let root = mkrepo("save");
    let buffer = "{\"mcpServers\":{\"x\":{\"command\":\"true\"}}}";
    let rec = apply_save(&root, buffer).expect("save must succeed");
    assert_eq!(rec.op, McpSaveOp::Write);
    assert!(rec.malformed.is_none(), "valid JSON must not be flagged");

    let path = repo_artifact(&root, MCP_CONFIG_FILE);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        buffer,
        "bytes equal the buffer"
    );
    assert!(
        git_in(&root, &["ls-files", MCP_CONFIG_FILE])
            .lines()
            .any(|l| l.trim() == MCP_CONFIG_FILE),
        "file must be git-tracked"
    );
    assert_eq!(
        git_in(&root, &["log", "-1", "--pretty=%s"]).trim(),
        UPDATE_SUBJECT
    );
    assert_eq!(
        git_in(&root, &["log", "-1", "--pretty=%an"]).trim(),
        "Kool.ad/e Planner"
    );

    let head = git_in(&root, &["rev-parse", "--short", "HEAD"])
        .trim()
        .to_string();
    let sha = rec.short_sha.expect("Write carries the checkpoint SHA");
    assert_eq!(sha.chars().count(), 7);
    assert_eq!(sha, head, "receipt SHA equals real git HEAD");
    assert_eq!(
        git_in(&root, &["status", "--porcelain"]).trim(),
        "",
        "tree porcelain-clean after the checkpoint"
    );
    let tmps: Vec<_> = fs::read_dir(root.join(crate::artifacts::CONFIG_DIR))
        .unwrap()
        .flat_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".koolade.tmp"))
        .collect();
    assert!(
        tmps.is_empty(),
        "atomic write leaves no temp residue: {tmps:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

/// AC leg 2 (runtime): blank save on an existing file removes it,
/// commits the CLEAR subject, and leaves the tree PORCELAIN-CLEAN
/// (the deletion is committed, not dangling).
#[test]
fn blank_save_removes_file_and_commits_clear_checkpoint_with_clean_tree() {
    let root = mkrepo("clear");
    let seed = "{\"mcpServers\":{\"x\":{\"command\":\"true\"}}}";
    assert_eq!(apply_save(&root, seed).unwrap().op, McpSaveOp::Write);
    let before_head = git_in(&root, &["rev-parse", "HEAD"]).trim().to_string();

    let rec = apply_save(&root, "   ").expect("blank save must succeed");
    assert_eq!(rec.op, McpSaveOp::Clear);
    let path = repo_artifact(&root, MCP_CONFIG_FILE);
    assert!(!path.is_file(), "file must be gone");
    assert_eq!(
        git_in(&root, &["log", "-1", "--pretty=%s"]).trim(),
        CLEAR_SUBJECT
    );
    assert_eq!(
        git_in(&root, &["log", "-1", "--pretty=%an"]).trim(),
        "Kool.ad/e Planner"
    );
    assert_ne!(
        git_in(&root, &["rev-parse", "HEAD"]).trim(),
        before_head,
        "clear must advance HEAD"
    );
    assert_eq!(
        git_in(&root, &["status", "--porcelain"]).trim(),
        "",
        "deletion committed, tree must be porcelain-clean"
    );

    let _ = fs::remove_dir_all(&root);
}

/// AC leg 4: an unedited (byte-identical) second Save is a TRUE no-op
/// against real git — HEAD identical, file mtime+bytes untouched, and
/// the receipt refuses to name a checkpoint.
#[test]
fn unchanged_save_is_zero_file_and_git_churn() {
    let root = mkrepo("unchanged");
    let buffer = "{\"stable\":true,\"pad\":\"xxxxxxxxxxxxxx\"}";
    apply_save(&root, buffer).expect("seed save");

    let path = repo_artifact(&root, MCP_CONFIG_FILE);
    let bytes_before = fs::read(&path).unwrap();
    let mtime_before = fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);
    let head_before = git_in(&root, &["rev-parse", "HEAD"]).trim().to_string();

    let rec = apply_save(&root, buffer).expect("second save");
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
    assert_eq!(
        git_in(&root, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "HEAD identical — zero commit churn (NFR-9)"
    );
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "");

    let _ = fs::remove_dir_all(&root);
}

/// D-16 end-to-end: a MALFORMED fragment is saved verbatim, the
/// checkpoint still lands under the UPDATE subject, the receipt flags
/// the probe error, and EXACTLY ONE commit was created.
#[test]
fn malformed_fragment_saves_verbatim_under_update_subject_with_receipt_flag() {
    let root = mkrepo("malformed");
    let fragment = "{\"broken\":";

    let rec = apply_save(&root, fragment).expect("malformed input must NOT block the save");
    assert_eq!(rec.op, McpSaveOp::Write);
    let err = rec.malformed.expect("malformed flag must be set");
    assert!(!err.trim().is_empty(), "flag carries the parse error text");

    assert_eq!(
        fs::read_to_string(repo_artifact(&root, MCP_CONFIG_FILE)).unwrap(),
        fragment,
        "file holds the fragment VERBATIM"
    );
    assert_eq!(
        git_in(&root, &["log", "-1", "--pretty=%s"]).trim(),
        UPDATE_SUBJECT
    );
    assert_eq!(
        git_in(&root, &["rev-list", "--count", "HEAD"]).trim(),
        "2",
        "exactly one new commit (init + this save)"
    );

    let _ = fs::remove_dir_all(&root);
}

/// AC leg 5: outside a git repository the disk write STILL LANDS and
/// the checkpoint failure surfaces as `AppError::Git` — disk-first,
/// error-second, never swallowed.
#[test]
fn non_git_repository_fails_at_checkpoint_with_disk_already_written() {
    let root = fresh_dir("nogit");
    let buffer = "{\"mcpServers\":{\"solo\":true}}";

    let err = apply_save(&root, buffer).expect_err("checkpoint must fail in a non-repo");
    assert!(
        matches!(err, AppError::Git { .. }),
        "expected AppError::Git, got: {err:?}"
    );

    let path = repo_artifact(&root, MCP_CONFIG_FILE);
    assert!(path.is_file(), "disk write preceded the checkpoint attempt");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        buffer,
        "buffered bytes landed despite the git failure"
    );

    let _ = fs::remove_dir_all(&root);
}
