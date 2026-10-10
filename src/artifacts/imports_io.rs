//! Document import into `.koolade-packet/planning/imports/` (SPECIFICATION.md §21).
//!
//! Imports are copied into the repository so Pi can inspect them as part of
//! normal project context. Text-oriented sources additionally gain a
//! Markdown companion file; binary sources are stored raw with a note.

use std::path::{Path, PathBuf};

use crate::artifacts::planning_store::{PlanningRoot, PlanningStore, StoreError};
use crate::artifacts::sanitize_basename;

/// Report of one successful import (surfaced in the UI as a toast/note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDoc {
    /// File name inside the canonical Koolade imports directory.
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

pub fn is_textual_import_name(name: &str) -> bool {
    extension_is_textual(name)
}

/// Copy `src` into the planning store's imports directory.
///
/// Guarantees:
/// * destination never collides with an existing file (numeric suffixing)
/// * `src` must lie outside both the code repository and planning store
/// * text sources additionally obtain a `<stem>.md` companion (identical bytes)
pub fn import_into_repo<R: PlanningRoot + ?Sized>(
    repo_root: &R,
    src: &Path,
) -> anyhow::Result<ImportedDoc> {
    let store = repo_root.planning_store();
    let expected_revision = store.revision()?;
    let code_root = repo_root
        .code_repository_root()
        .unwrap_or_else(|| repo_root.planning_layout().root().to_path_buf());
    import_into_store(&store, &code_root, src, &expected_revision).map(|(doc, _)| doc)
}

/// Import using the connected code checkout and planning-store roots for
/// containment checks and an optimistic revision for the durable write.
pub fn import_into_store(
    store: &PlanningStore,
    code_repository_root: &Path,
    src: &Path,
    expected_revision: &str,
) -> anyhow::Result<(ImportedDoc, String)> {
    let src = std::fs::canonicalize(src)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", src.display()))?;
    let repo = std::fs::canonicalize(code_repository_root)
        .map_err(|e| anyhow::anyhow!("cannot resolve repo: {e}"))?;
    if src.starts_with(&repo) {
        anyhow::bail!("the source file is already inside the repository; point at an outside path");
    }
    match std::fs::canonicalize(&store.root) {
        Ok(planning_root) if src.starts_with(&planning_root) => {
            anyhow::bail!(
                "the source file is already inside the planning store; point at an outside path"
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => anyhow::bail!("cannot resolve planning store: {error}"),
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
    // Unique destination name: base.ext, base-1.ext, base-2.ext, ...
    let (stem_part, ext_part) = match base.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (base.clone(), String::new()),
    };
    let textual = extension_is_textual(&base);
    let already_markdown = base.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    });
    let raw_bytes = std::fs::read(&src)?;
    let companion_bytes = (textual && !already_markdown).then_some(&raw_bytes);
    let mut suffix = 0usize;
    loop {
        let candidate_stem = if suffix == 0 {
            stem_part.clone()
        } else {
            format!("{stem_part}-{suffix}")
        };
        let candidate = format!("{candidate_stem}{ext_part}");
        let relative = format!(
            "{}/{}",
            crate::artifacts::planning_store::paths::IMPORTS,
            candidate
        );
        match store.read(&relative) {
            Ok(_) => {
                suffix += 1;
                continue;
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            }
            Err(error) => return Err(error.into()),
        }

        let mut companion = None;
        let mut note = None;
        let mut changes = vec![(relative, raw_bytes.clone())];
        if let Some(bytes) = &companion_bytes {
            if std::str::from_utf8(bytes).is_err() {
                note = Some(
                    "Source carried a textual extension but non-UTF-8 content; stored raw.".into(),
                );
            } else {
                let extension = base
                    .rsplit_once('.')
                    .map(|(_, extension)| extension.to_ascii_lowercase())
                    .unwrap_or_default();
                let comp_name = format!("{candidate_stem}-{extension}.md");
                let companion_relative = format!(
                    "{}/{}",
                    crate::artifacts::planning_store::paths::IMPORTS,
                    comp_name
                );
                match store.read(&companion_relative) {
                    Ok(_) => {
                        suffix += 1;
                        continue;
                    }
                    Err(StoreError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                changes.push((companion_relative, bytes.to_vec()));
                companion = Some(comp_name);
            }
        } else if !textual {
            note = Some(
                "Binary document stored as-is; the planning agent may struggle with it — prefer text/Markdown exports where possible."
                    .into(),
            );
        }
        let (_, revision) = store.transaction_with_revision(&changes, Some(expected_revision))?;
        return Ok((
            ImportedDoc {
                stored_name: candidate,
                companion,
                note,
            },
            revision,
        ));
    }
}

/// Lightweight listing for the UI / context builder (name + size).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub name: String,
    pub bytes: u64,
}

pub fn list_imports<R: PlanningRoot + ?Sized>(repo_root: &R) -> Vec<ImportEntry> {
    list_imports_in_store(&repo_root.planning_store()).unwrap_or_default()
}

pub fn list_imports_in_store(store: &PlanningStore) -> Result<Vec<ImportEntry>, StoreError> {
    Ok(store
        .list_files(crate::artifacts::planning_store::paths::IMPORTS)?
        .into_iter()
        .map(|entry| ImportEntry {
            name: entry.name,
            bytes: entry.bytes,
        })
        .collect())
}

/// Path helper reused elsewhere (e.g. the context builder advertises imports).
pub fn imports_dir<R: PlanningRoot + ?Sized>(repo_root: &R) -> PathBuf {
    repo_root.planning_layout().imports_root()
}

#[cfg(test)]
mod tests;
