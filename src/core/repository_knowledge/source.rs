use std::{collections::BTreeMap, path::Path};

mod git;
pub(crate) use git::{
    command_bytes, command_bytes_bounded, command_bytes_with_deadline, command_text, valid_oid,
    valid_path,
};

mod classify;
pub(crate) use classify::{is_doc, is_manifest, is_test, source_extension};

use sha2::{Digest, Sha256};

use super::{EvidenceKind, EvidenceRefV1, ScanOmissionV1};

pub(super) const MAX_TREE_ENTRIES: usize = 4_096;
pub(super) const MAX_TOTAL_BYTES: usize = 1024 * 1024;
pub(super) const MAX_BLOB_BYTES: usize = 64 * 1024;
const MAX_TREE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 8;
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "out",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".turbo",
    ".cache",
];

#[derive(Clone, Debug)]
pub(super) struct TreeFile {
    pub path: String,
    pub object_id: String,
    pub mode: String,
}

#[derive(Default)]
pub(super) struct TreeScan {
    pub files: BTreeMap<String, TreeFile>,
    pub top_directories: BTreeMap<String, Vec<String>>,
    pub entries_seen: usize,
    pub truncated: bool,
    pub omissions: Vec<ScanOmissionV1>,
}

pub(super) struct BlobReader<'a> {
    root: &'a Path,
    commit_sha: &'a str,
    files: &'a BTreeMap<String, TreeFile>,
    total_bytes: usize,
}

impl<'a> BlobReader<'a> {
    pub fn new(root: &'a Path, commit_sha: &'a str, files: &'a BTreeMap<String, TreeFile>) -> Self {
        Self {
            root,
            commit_sha,
            files,
            total_bytes: 0,
        }
    }

    pub fn read(&mut self, path: &str) -> Result<(String, EvidenceRefV1), String> {
        if !valid_path(path) {
            return Err("repository path is invalid".into());
        }
        let Some(file) = self.files.get(path) else {
            return Err("repository path is not in the pinned tree".into());
        };
        if !matches!(file.mode.as_str(), "100644" | "100755") {
            return Err("repository object is not a regular file".into());
        }
        let size = command_text(self.root, &["cat-file", "-s", &file.object_id])?;
        let size = size
            .trim()
            .parse::<usize>()
            .map_err(|_| "Git returned an invalid blob size".to_owned())?;
        if size > MAX_BLOB_BYTES || self.total_bytes.saturating_add(size) > MAX_TOTAL_BYTES {
            return Err("repository scan byte budget exhausted".into());
        }
        let bytes = command_bytes(self.root, &["cat-file", "blob", &file.object_id])?;
        if bytes.len() != size {
            return Err("pinned repository blob changed while being read".into());
        }
        self.total_bytes += size;
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let content = String::from_utf8(bytes)
            .map_err(|_| "repository blob is not valid UTF-8".to_owned())?;
        Ok((
            content,
            EvidenceRefV1 {
                path: path.to_owned(),
                symbol: None,
                line_start: None,
                line_end: None,
                content_sha256: sha,
                source_sha: self.commit_sha.to_owned(),
                kind: kind_for(path),
            },
        ))
    }

    pub fn bytes_read(&self) -> usize {
        self.total_bytes
    }
}

pub(super) fn tree(root: &Path, commit_sha: &str) -> Result<TreeScan, String> {
    let root_tree = command_text(
        root,
        &["rev-parse", "--verify", &format!("{commit_sha}^{{tree}}")],
    )?;
    let mut pending = vec![(String::new(), root_tree.trim().to_owned(), 0usize)];
    let mut result = TreeScan::default();
    while let Some((prefix, tree_oid, depth)) = pending.pop() {
        if result.entries_seen >= MAX_TREE_ENTRIES {
            result.truncated = true;
            break;
        }
        let output = match command_bytes_bounded(
            root,
            &["ls-tree", "-z", &tree_oid],
            MAX_TREE_RESPONSE_BYTES,
        ) {
            Ok(output) => output,
            Err(_) => {
                result.truncated = true;
                result.omissions.push(ScanOmissionV1 {
                    path: if prefix.is_empty() { "." } else { &prefix }.into(),
                    reason: "directory listing exceeded the bounded response size".into(),
                });
                continue;
            }
        };
        let mut children = Vec::new();
        for record in output
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            if result.entries_seen >= MAX_TREE_ENTRIES {
                result.truncated = true;
                break;
            }
            result.entries_seen += 1;
            let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
                result.truncated = true;
                continue;
            };
            let metadata = String::from_utf8_lossy(&record[..tab]);
            let mut fields = metadata.split_ascii_whitespace();
            let mode = fields.next().unwrap_or_default();
            let kind = fields.next().unwrap_or_default();
            let oid = fields.next().unwrap_or_default();
            let name = match std::str::from_utf8(&record[tab + 1..]) {
                Ok(value) if !value.is_empty() && !value.contains('/') => value,
                _ => {
                    result.omissions.push(ScanOmissionV1 {
                        path: prefix.clone(),
                        reason: "non-UTF-8 or malformed Git tree entry was omitted".into(),
                    });
                    continue;
                }
            };
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            if kind == "tree" {
                if skip_dir(name) || depth >= MAX_TREE_DEPTH {
                    result.omissions.push(ScanOmissionV1 {
                        path,
                        reason: if depth >= MAX_TREE_DEPTH {
                            "maximum tree depth reached".into()
                        } else {
                            "generated or vendored directory skipped".into()
                        },
                    });
                    continue;
                }
                if !valid_oid(oid) {
                    result.truncated = true;
                    continue;
                }
                children.push((path, oid.to_owned(), depth + 1));
            } else if kind == "blob" && valid_oid(oid) {
                if !valid_path(&path) || !matches!(mode, "100644" | "100755") {
                    result.omissions.push(ScanOmissionV1 {
                        path,
                        reason: "unsupported or unsafe Git entry omitted".into(),
                    });
                    continue;
                }
                if let Some((directory, _)) = path.split_once('/') {
                    result
                        .top_directories
                        .entry(directory.to_owned())
                        .or_default()
                        .push(path.clone());
                }
                result.files.insert(
                    path.clone(),
                    TreeFile {
                        path,
                        object_id: oid.to_owned(),
                        mode: mode.to_owned(),
                    },
                );
            }
        }
        children.reverse();
        pending.extend(children);
    }
    if result.omissions.len() > 128 {
        result.omissions.truncate(128);
    }
    Ok(result)
}

fn skip_dir(name: &str) -> bool {
    if SKIP_DIRS.contains(&name) {
        return true;
    }
    name.starts_with('.') && !matches!(name, ".github" | ".gitlab" | ".circleci")
}

fn kind_for(path: &str) -> EvidenceKind {
    if is_manifest(path) {
        EvidenceKind::Manifest
    } else if is_doc(path) {
        EvidenceKind::Documentation
    } else if is_test(path) {
        EvidenceKind::Test
    } else {
        EvidenceKind::Source
    }
}
