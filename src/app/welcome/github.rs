use crate::core::gitops;
use crate::error::AppError;
use std::path::{Path, PathBuf};
/// A parsed GitHub clone target.
///
/// `url` is the canonical rebuilt form, never the raw pasted string, so
/// query, userinfo, and host quirks cannot leak into git argv.
pub struct GithubTarget {
    /// Owner as pasted.
    pub owner: String,
    /// Repository as pasted (`.git` suffix and trailing `/` stripped).
    pub repo: String,
    /// `https://github.com/{owner}/{repo}.git` (lowercase scheme + host).
    pub url: String,
}

/// Parse an `https://github.com/{owner}/{repo}` URL the connect card can
/// clone. Std-only (no url/regex crate, per the frozen manifest):
/// trim; require an `https` scheme (case-insensitive); require the
/// authority to be exactly `github.com` (case-insensitive) with NO
/// userinfo, port, query, or fragment anywhere in the remainder; require
/// the remainder to be exactly `/{owner}/{repo}`, optionally terminated by
/// a single `/` or a case-insensitive `.git`; each segment non-empty,
/// `..`-free, ASCII `[A-Za-z0-9._-]`, starting and ending alphanumeric.
///
/// Every rejection is a distinct operator-actionable sentence that names
/// the canonical shape.
pub fn parse_github_url(raw: &str) -> Result<GithubTarget, String> {
    const SHAPE: &str = "https://github.com/{owner}/{repo}";
    const HINT: &str =
        "Paste a github.com URL to clone, e.g. https://github.com/octocat/hello-world";

    let url = raw.trim();
    if url.is_empty() {
        return Err(HINT.to_string());
    }
    // `to_ascii_lowercase` preserves BYTE LENGTH, so indices discovered in
    // the lowered view address the original view 1:1 (segment case stays
    // intact for the rebuilt target).
    let folded_full = url.to_ascii_lowercase();
    let folded: &str = folded_full.as_str();
    let rest = match folded.strip_prefix("https://") {
        Some(rest) => rest,
        None => {
            if folded.starts_with("git@") {
                return Err(format!(
                    "That is the SSH form \u{2014} use the HTTPS one instead: paste {SHAPE} (no git@ prefix)."
                ));
            }
            if let Some(at) = folded.find("://") {
                let scheme: String = folded[..at]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    .collect();
                if !scheme.is_empty() {
                    return Err(format!(
                        "Only {SHAPE} URLs can be cloned here (got scheme '{scheme}')."
                    ));
                }
            }
            return Err(HINT.to_string());
        }
    };
    if rest.contains('?') || rest.contains('#') {
        return Err(format!(
            "Strip any \u{2018}?\u{2026}\u{2019} and \u{2018}#\u{2026}\u{2019} extras \u{2014} paste a plain {SHAPE} URL (an optional .git suffix is fine)."
        ));
    }
    // Authority bounds (relative to `rest`) and the path part's absolute
    // start (inclusive of its leading '/', or rest's end when absent).
    let (auth_len, path_at) = match rest.find('/') {
        Some(rel) => (rel, 8 + rel),
        None => (rest.len(), 8 + rest.len()),
    };
    let authority_folded = &rest[..auth_len];
    if authority_folded.is_empty() {
        return Err(format!(
            "Only {SHAPE} URLs can be cloned here (nothing after \u{2018}https://\u{2019} looked like a host)."
        ));
    }
    if authority_folded.contains('@') {
        return Err(format!(
            "Drop the \u{2018}user:token@\u{2019} login \u{2014} paste {SHAPE} plainly (credentials are not supported here)."
        ));
    }
    if authority_folded.contains(':') {
        return Err(format!(
            "Drop the port number \u{2014} GitHub sits on the default HTTPS port, so paste {SHAPE}."
        ));
    }
    if authority_folded != "github.com" {
        return Err(format!(
            "Only {SHAPE} URLs can be cloned here (got {}).",
            &url[8..8 + auth_len]
        ));
    }
    // Trim the path part — ONE leading '/', at most ONE trailing '/', and a
    // terminal case-insensitive ".git" — tracking ABSOLUTE bounds, so the
    // surviving segment range can be re-applied to the ORIGINAL url.
    let mut lo = path_at;
    let mut hi = 8 + rest.len();
    if lo < hi && folded.as_bytes()[lo] == b'/' {
        lo += 1;
    }
    if hi > lo && folded.as_bytes()[hi - 1] == b'/' {
        hi -= 1;
    }
    const DOT_GIT: &str = ".git";
    if hi - lo > DOT_GIT.len() && folded[hi - DOT_GIT.len()..hi].eq_ignore_ascii_case(DOT_GIT) {
        hi -= DOT_GIT.len();
    }
    // Partition on '/' (walked per-char, so every bound is a UTF-8
    // boundary), then validate each part against the segment rules.
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut start = lo;
    for (rel_i, ch) in folded[lo..hi].char_indices() {
        if ch == '/' {
            parts.push((start, lo + rel_i));
            start = lo + rel_i + 1;
        }
    }
    parts.push((start, hi));
    if parts.len() == 1 {
        return Err(format!(
            "An owner alone does not name a repository \u{2014} paste {SHAPE}."
        ));
    }
    if parts.len() != 2 {
        return Err(format!(
            "Too many parts \u{2014} paste exactly {SHAPE} (no branch names; an optional .git suffix is fine)."
        ));
    }
    for &(a, b) in &parts {
        if !valid_repo_segment(&folded[a..b]) {
            return Err(format!(
                "Owner and repo names must start and end with a letter or digit, using only letters, digits, \u{2018}-_\u{2019} and dots (no slashes) \u{2014} paste {SHAPE}."
            ));
        }
    }
    // Slice the segments OUT OF THE ORIGINAL URL — operator casing intact.
    let owner = &url[parts[0].0..parts[0].1];
    let repo = &url[parts[1].0..parts[1].1];
    Ok(GithubTarget {
        owner: owner.to_string(),
        repo: repo.to_string(),
        url: format!("https://github.com/{owner}/{repo}.git"),
    })
}

/// `[A-Za-z0-9._-]+`, first and last byte alphanumeric, no `..` runs.
fn valid_repo_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    !bytes.is_empty()
        && !segment.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
}

/// Where a clone lands: `$HOME/kool-ade-workspaces/{repo}`. Pure (takes `home`) so tests can
/// inject a sandboxed home; the segment arrives in the operator's original
/// case.
pub fn clone_destination(repo: &str, home: &Path) -> PathBuf {
    home.join("kool-ade-workspaces").join(repo)
}

/// Clone `source` (ALWAYS the canonical url rebuilt by
/// [`parse_github_url`]) into `$HOME/kool-ade-workspaces/{repo}` via the system git CLI.
///
/// Scratch-plus-rename invariant: git writes into
/// `$HOME/kool-ade-workspaces/{repo}.koolade-cloning.<pid>` and the finished tree is renamed
/// into place atomically within $HOME \u{2014} the real destination can
/// never exist half-written, even if the app or the machine dies mid-
/// download. Stale scratch left by crashed attempts for the same repo is
/// swept before every new clone (the sweep predicate and the scratch name
/// stay coupled in this one function).
pub fn perform_clone(source: &str, repo: &str) -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Io {
            op: "locate home directory".into(),
            detail: "Set $HOME to a writable directory before cloning.".into(),
        })?;
    perform_clone_at(source, repo, &home)
}

/// The home-independent core of [`perform_clone`]: destinations, the
/// stale-scratch sweep and the atomic rename all key off `home`, which
/// production resolves from `$HOME` and tests inject directly (no env
/// mutation).
pub fn perform_clone_at(source: &str, repo: &str, home: &Path) -> Result<PathBuf, AppError> {
    let dest = clone_destination(repo, home);
    if dest.is_file() {
        return Err(AppError::InvalidRepo {
            path: dest.to_string_lossy().into_owned(),
            detail: "a file already occupies that name".into(),
        });
    }
    if dest.is_dir() {
        // Already cloned \u{2014} fast-path back through the EXISTING connect
        // path (or let attempt_connect raise its incumbent "no .git
        // directory found" banner for a foreign directory). Never re-clone.
        return Ok(dest);
    }
    if !home.is_dir() {
        return Err(AppError::Io {
            op: "prepare clone parent".into(),
            detail: format!(
                "$HOME points at {}, which is not a directory",
                home.to_string_lossy()
            ),
        });
    }
    let parent = dest.parent().expect("clone destination has a parent");
    if let Err(e) = std::fs::create_dir_all(parent) {
        return Err(AppError::Io {
            op: "prepare clone parent".into(),
            detail: format!("could not create {}: {e}", parent.to_string_lossy()),
        });
    }
    let scratch_prefix = format!("{repo}.koolade-cloning.");
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&scratch_prefix)
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let scratch = parent.join(format!("{repo}.koolade-cloning.{}", std::process::id()));
    if let Err(e) = gitops::clone_repo(source, &scratch) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(e);
    }
    if !scratch.is_dir() {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(AppError::Git {
            cmd: crate::error::redact_secrets(&format!("clone {source}")),
            detail: "git reported success but the scratch directory is missing".into(),
        });
    }
    if let Err(e) = std::fs::rename(&scratch, &dest) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(AppError::Io {
            op: "place cloned repository".into(),
            detail: format!("could not move the clone into place: {e}"),
        });
    }
    Ok(dest)
}
