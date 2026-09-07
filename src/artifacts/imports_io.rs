//! Document import into `planning/imports/` (SPECIFICATION.md §21).
//!
//! Imports are copied into the repository so Pi can inspect them as part of
//! normal project context. Text-oriented sources additionally gain a
//! Markdown companion file; binary sources are stored raw with a note.

use std::path::{Path, PathBuf};

use crate::artifacts::{repo_artifact, sanitize_basename, IMPORTS_DIR};

/// Report of one successful import (surfaced in the UI as a toast/note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDoc {
    /// File name inside `planning/imports/`.
    pub stored_name: String,
    /// Companion `.md` extraction file name, if produced.
    pub companion: Option<String>,
    /// Advisory note, e.g. binary payloads cannot be text-extracted in MVP.
    pub note: Option<String>,
}

/// Extensions treated as text for companion-Markdown production.
const TEXTUAL_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "text", "rst", "csv", "tsv", "json", "yaml", "yml", "toml", "ini",
    "log", "xml", "html", "sql", "rs", "py", "js", "ts", "go", "java", "c", "h", "cpp", "hpp",
    "sh", "bat", "ps1", "proto", "graphql",
];

fn extension_is_textual(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    TEXTUAL_EXTENSIONS.contains(&ext.as_str())
}

/// Copy `src` into the repository's imports directory.
///
/// Guarantees:
/// * destination never collides with an existing file (numeric suffixing)
/// * `src` must lie OUTSIDE the repository (prevents self-copy cycles)
/// * text sources additionally obtain a `<stem>.md` companion (identical bytes)
pub fn import_into_repo(repo_root: &Path, src: &Path) -> anyhow::Result<ImportedDoc> {
    let src = std::fs::canonicalize(src)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", src.display()))?;
    let repo = std::fs::canonicalize(repo_root)
        .map_err(|e| anyhow::anyhow!("cannot resolve repo: {e}"))?;
    if src.starts_with(&repo) {
        anyhow::bail!("the source file is already inside the repository; point at an outside path");
    }

    let metadata = std::fs::metadata(&src)?;
    if !metadata.is_file() {
        anyhow::bail!("only regular files can be imported");
    }

    let stem = src
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "import".into());
    let base = sanitize_basename(&stem);
    let imports = repo_artifact(repo_root, IMPORTS_DIR);
    std::fs::create_dir_all(&imports)?;

    // Unique destination name: base.ext, base-1.ext, base-2.ext, ...
    let (stem_part, ext_part) = match base.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (base.clone(), String::new()),
    };
    let mut candidate = format!("{stem_part}{ext_part}");
    let mut n = 1usize;
    while imports.join(&candidate).exists() {
        candidate = format!("{stem_part}-{n}{ext_part}");
        n += 1;
    }
    let dest = imports.join(candidate.clone());
    std::fs::copy(&src, &dest)?;

    let (companion, note) = if extension_is_textual(&base) {
        let already_md = base
            .rsplit_once('.')
            .is_some_and(|(_, e)| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
        if already_md {
            (None, None)
        } else {
            let comp_name = format!(
                "{}-{}.md",
                stem_part,
                base.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
            );
            let bytes = std::fs::read(&src)?;
            let comp_path = imports.join(comp_name.clone());
            if std::str::from_utf8(&bytes).is_ok() {
                std::fs::write(&comp_path, bytes)?;
                (Some(comp_name), None)
            } else {
                (
                    None,
                    Some("Source carried a textual extension but non-UTF-8 content; stored raw.".into()),
                )
            }
        }
    } else {
        (
            None,
            Some(
                "Binary document stored as-is; the planning agent may struggle with it — prefer text/Markdown exports where possible."
                    .into(),
            ),
        )
    };

    // Defensive: verify the write landed.
    if !dest.exists() {
        anyhow::bail!("import vanished after copy (disk full?)");
    }
    Ok(ImportedDoc {
        stored_name: candidate,
        companion,
        note,
    })
}

/// Lightweight listing for the UI / context builder (name + size).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub name: String,
    pub bytes: u64,
}

pub fn list_imports(repo_root: &Path) -> Vec<ImportEntry> {
    let dir = repo_artifact(repo_root, IMPORTS_DIR);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<ImportEntry> = read
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if !md.is_file() {
                return None;
            }
            Some(ImportEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                bytes: md.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Path helper reused elsewhere (e.g. the context builder advertises imports).
pub fn imports_dir(repo_root: &Path) -> PathBuf {
    repo_artifact(repo_root, IMPORTS_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(prefix: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("packet_imp_{prefix}_{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("packet_outside_{prefix}_{}", std::process::id()));
        for d in [&root, &outside] {
            let _ = std::fs::remove_dir_all(d);
            std::fs::create_dir_all(d).unwrap();
        }
        (root, outside)
    }

    #[test]
    fn textual_source_gets_companion_and_collision_numbering() {
        let (repo, out) = sandbox("txt");
        let src = out.join("notes.txt");
        std::fs::write(&src, "hello\nworld\n").unwrap();
        let r1 = import_into_repo(&repo, &src).unwrap();
        let r2 = import_into_repo(&repo, &src).unwrap();
        assert_eq!(r1.stored_name, "notes.txt");
        assert_eq!(r1.companion.as_deref(), Some("notes-txt.md"));
        assert!(r1.note.is_none());
        assert_eq!(r2.stored_name, "notes-1.txt");
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn markdown_sources_do_not_duplicate_themselves() {
        let (repo, out) = sandbox("md");
        let src = out.join("design.md");
        std::fs::write(&src, "# hi\n").unwrap();
        let r = import_into_repo(&repo, &src).unwrap();
        assert_eq!(r.stored_name, "design.md");
        assert!(r.companion.is_none());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn binary_sources_are_flagged_but_kept() {
        let (repo, out) = sandbox("bin");
        let src = out.join("diagram.bin");
        std::fs::write(&src, vec![0xFF, 0xD8, 0x00]).unwrap();
        let r = import_into_repo(&repo, &src).unwrap();
        assert_eq!(r.stored_name, "diagram.bin");
        assert!(r.note.is_some());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn refuses_self_copy_into_same_repo() {
        let (repo, out) = sandbox("self");
        std::fs::create_dir_all(repo.join(IMPORTS_DIR)).unwrap();
        let inside = repo.join("planning").join("imports").join("a.txt");
        std::fs::write(&inside, "x").unwrap();
        assert!(import_into_repo(&repo, &inside).is_err());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn list_reports_sorted_entries() {
        let (repo, _out) = sandbox("list");
        std::fs::create_dir_all(repo.join(IMPORTS_DIR)).unwrap();
        std::fs::write(repo.join(IMPORTS_DIR).join("b.md"), "bb").unwrap();
        std::fs::write(repo.join(IMPORTS_DIR).join("a.md"), "a").unwrap();
        let v = list_imports(&repo);
        assert_eq!(v.iter().map(|e| e.name.clone()).collect::<Vec<_>>(), vec!["a.md", "b.md"]);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
