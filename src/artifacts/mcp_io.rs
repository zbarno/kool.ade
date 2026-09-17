//! Load/save/remove/checkpoint lifecycle for `.planner/mcp.json` (F-18,
//! D-16). The planner never interprets the file's schema — it is a raw
//! byte store advertised verbatim to every pi session — so all judgment
//! here is syntactic (JSON well-formedness probe only) and structural
//! (blank ⇔ unconfigured, matching the verified `context_build` filter).

use std::path::Path;

use crate::artifacts::{MCP_CONFIG_FILE, atomic_write, repo_artifact};
use crate::core::gitops;
use crate::error::AppError;

/// Dedicated checkpoint subject for a Write save (NFR-9: short imperative;
/// parallels the existing "settings: update stakeholders and identity").
pub const UPDATE_SUBJECT: &str = "settings: update mcp server configuration";
/// Dedicated checkpoint subject for a blank-save removal.
pub const CLEAR_SUBJECT: &str = "settings: clear mcp server configuration";

/// What a `load_state` call knows about the file on disk.
pub struct McpLoadState {
    /// `.planner/mcp.json` exists as a regular file.
    pub present: bool,
    /// File contents (UTF-8), populated only when `present`.
    pub content: Option<String>,
    /// Human-readable read failure when the file exists but could not be
    /// read (permissions, non-UTF-8 bytes, …). Mutually exclusive with
    /// `content`.
    pub read_error: Option<String>,
}

/// Inspect the on-disk file without touching git. Reads are lossy-tolerant:
/// any IO failure degrades to `read_error`, never panics and never blocks.
pub fn load_state(root: &Path) -> McpLoadState {
    let path = repo_artifact(root, MCP_CONFIG_FILE);
    let present = path.is_file();
    let mut content = None;
    let mut read_error = None;
    if present {
        match std::fs::read_to_string(&path) {
            Ok(text) => content = Some(text),
            Err(e) => read_error = Some(e.to_string()),
        }
    }
    McpLoadState {
        present,
        content,
        read_error,
    }
}

/// Syntax-only JSON well-formedness probe (NFR-5): the app deliberately
/// enforces NO server schema — the file's consumers sit outside the planner.
/// Any top-level JSON value (object, array, scalar, even `5`) passes.
pub fn probe_json(text: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Which disk action a Save implies. Derived purely, so the dialog layer
/// and the tests share one rule table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSaveOp {
    /// No disk change warranted: no file & blank buffer, OR the buffer is
    /// BYTE-EXACTLY identical to the stored content (no normalization —
    /// cosmetic differences always rewrite).
    Unchanged,
    /// Write the buffer atomically and checkpoint under UPDATE_SUBJECT.
    Write,
    /// Remove the file (blank ⇒ unconfigured) and checkpoint under
    /// CLEAR_SUBJECT; also purges blank-residue files.
    Clear,
}

/// Four-way rule table for a Save. `current` is the on-disk content (only
/// meaningful when `file_present`).
pub fn classify(file_present: bool, current: Option<&str>, buffer: &str) -> McpSaveOp {
    let blank = buffer.trim().is_empty();
    if blank {
        if file_present {
            McpSaveOp::Clear
        } else {
            McpSaveOp::Unchanged
        }
    } else if file_present && current == Some(buffer) {
        McpSaveOp::Unchanged
    } else {
        McpSaveOp::Write
    }
}

/// Outcome of an `apply_save`, consumed by the dialog to pick toast,
/// feedback line, and keep-open behaviour.
#[derive(Debug)]
pub struct McpApplyReceipt {
    /// The operation actually performed.
    pub op: McpSaveOp,
    /// Short (7-char) checkpoint SHA, `None` for `Unchanged` (no commit).
    pub short_sha: Option<String>,
    /// JSON probe failure text when a `Write` landed on malformed input
    /// — informational only; the save proceeded (D-16 non-blocking rule).
    pub malformed: Option<String>,
}

/// Execute a Save against `root`: classify, then either no-op, atomic-write
/// + checkpoint, or remove + checkpoint. Disk effects ALWAYS precede the
/// git effect, so a checkpoint failure never strands an un-written change
/// — and never swallows: the `AppError` propagates to the dialog.
pub fn apply_save(root: &Path, buffer: &str) -> Result<McpApplyReceipt, AppError> {
    // Writer section: config write + checkpoint shares the planning index.
    let _guard = crate::core::writer_gate::acquire();
    let st = load_state(root);
    let op = classify(st.present, st.content.as_deref(), buffer);
    match op {
        McpSaveOp::Unchanged => Ok(McpApplyReceipt {
            op,
            short_sha: None,
            malformed: None,
        }),
        McpSaveOp::Write => {
            // Computed for the receipt, never fatal (D-16: warn, don't block).
            let malformed = probe_json(buffer).err();
            let path = repo_artifact(root, MCP_CONFIG_FILE);
            atomic_write(&path, buffer).map_err(|e| AppError::Io {
                op: "write .planner/mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            let sha = gitops::commit(root, UPDATE_SUBJECT, &[MCP_CONFIG_FILE.to_string()])?;
            Ok(McpApplyReceipt {
                op,
                short_sha: Some(short_sha(&sha)),
                malformed,
            })
        }
        McpSaveOp::Clear => {
            let path = repo_artifact(root, MCP_CONFIG_FILE);
            std::fs::remove_file(&path).map_err(|e| AppError::Io {
                op: "remove .planner/mcp.json".to_string(),
                detail: e.to_string(),
            })?;
            let sha = gitops::commit(root, CLEAR_SUBJECT, &[MCP_CONFIG_FILE.to_string()])?;
            Ok(McpApplyReceipt {
                op,
                short_sha: Some(short_sha(&sha)),
                malformed: None,
            })
        }
    }
}

/// First 7 chars of a git short SHA (git may yield fewer digits on young
/// histories; callers treat the result as an opaque label).
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    fn fresh_dir(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("packet_mcp_{label}_{}", std::process::id()));
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

    /// Positive table: ANY syntactically valid JSON passes, schema blind.
    /// Top-level `5` is legal JSON and MUST pass — the consumer's schema
    /// is outside the planner (D-16).
    #[test]
    fn probe_accepts_objects_arrays_scalars_and_unicode_keys() {
        for sample in [
            "{\"mcpServers\": {\"x\": {\"command\": \"true\"}}}",
            "[1, 2, 3]",
            "5",
            "\"quoted\"",
            "null",
            "{\"uniçødé κey\": {\"в\": true}}",
        ] {
            assert_eq!(probe_json(sample), Ok(()), "sample: {sample}");
        }
    }

    /// Negative table: every entry is ill-formed syntax and yields a
    /// NON-EMPTY human message (the dialog quotes it verbatim).
    #[test]
    fn probe_rejects_truncation_trailing_comma_prose_and_double_docs() {
        for sample in [
            "{\"servers\":",        // truncation
            "{ \"a\": 1, }",        // trailing comma
            "not json",             // plain prose
            "{\"a\":1}\n{\"b\":2}", // double document
        ] {
            let err = probe_json(sample)
                .err()
                .unwrap_or_else(|| panic!("expected Err for: {sample}"));
            assert!(
                !err.trim().is_empty(),
                "probe message must be non-empty for: {sample}"
            );
        }
    }

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
            "Packet Planner"
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
        let tmps: Vec<_> = fs::read_dir(root.join(".planner"))
            .unwrap()
            .flat_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".packet.tmp"))
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
            "Packet Planner"
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
}
