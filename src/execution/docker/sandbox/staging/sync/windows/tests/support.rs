use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::super::super::super::archive::{Fingerprint, Manifest, ReturnedEntry, hash};
use super::super::Checkpoint;

pub(super) const BYTE_LIMIT: u64 = 1024 * 1024;

pub(super) fn returned(path: &str, bytes: &[u8]) -> BTreeMap<PathBuf, ReturnedEntry> {
    returned_mode(path, bytes, 0o644)
}

pub(super) fn returned_mode(
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> BTreeMap<PathBuf, ReturnedEntry> {
    BTreeMap::from([(
        PathBuf::from(path),
        ReturnedEntry {
            fingerprint: Fingerprint::File {
                sha256: hash(bytes),
                size: bytes.len() as u64,
                mode,
            },
            bytes: bytes.to_vec(),
            mode,
        },
    )])
}

fn file_fingerprint(bytes: &[u8], mode: u32) -> Fingerprint {
    Fingerprint::File {
        sha256: hash(bytes),
        size: bytes.len() as u64,
        mode,
    }
}

pub(super) fn baseline(path: &str, bytes: &[u8]) -> Manifest {
    baseline_mode(path, bytes, 0o644)
}

pub(super) fn baseline_mode(path: &str, bytes: &[u8], mode: u32) -> Manifest {
    BTreeMap::from([(PathBuf::from(path), file_fingerprint(bytes, mode))])
}

pub(super) fn apply_test(
    root: &Path,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
    source_output_paths: Option<&[String]>,
    hook: impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    initialize_git(root);
    apply_initialized_test(root, baseline, returned, source_output_paths, hook)
}

pub(super) fn apply_initialized_test(
    root: &Path,
    baseline: &Manifest,
    returned: &BTreeMap<PathBuf, ReturnedEntry>,
    source_output_paths: Option<&[String]>,
    hook: impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    let identity = identity(root);
    let git_identity = identity(&root.join(".git"));
    super::super::apply_with_hook(
        root,
        &identity,
        &git_identity,
        baseline,
        returned,
        BYTE_LIMIT,
        source_output_paths,
        hook,
    )
}

pub(super) fn initialize_git(root: &Path) {
    let initialized = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet", "--template="])
        .status()
        .expect("Git is available for Windows stage-out tests");
    assert!(initialized.success());
    let added = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=",
            "-c",
            "core.excludesFile=",
            "-c",
            "core.autocrlf=false",
            "add",
            "-A",
        ])
        .status()
        .expect("Git can prepare the local stage-out test index");
    assert!(added.success());
}

pub(super) fn git_index_executable(root: &Path, path: &str) -> Option<bool> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["--literal-pathspecs", "ls-files", "--stage", "-z"])
        .output()
        .expect("Git can inspect the local test index");
    assert!(output.status.success());
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .find_map(|record| {
            let separator = record.iter().position(|byte| *byte == b'\t')?;
            let entry_path = std::str::from_utf8(&record[separator + 1..]).ok()?;
            (entry_path == path).then(|| record.starts_with(b"100755 "))
        })
}

pub(super) fn set_git_index_executable(root: &Path, path: &str, executable: bool) {
    let chmod = if executable {
        "--chmod=+x"
    } else {
        "--chmod=-x"
    };
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "--literal-pathspecs",
            "update-index",
            chmod,
            "--",
            path,
        ])
        .status()
        .expect("Git can set the local test index mode");
    assert!(status.success());
}

pub(super) fn identity(
    root: &Path,
) -> crate::execution::docker::sandbox::workspace_identity::WorkspaceObjectId {
    let metadata = fs::metadata(root).unwrap();
    crate::execution::docker::sandbox::workspace_identity::from_metadata(&metadata, root)
        .expect("temporary workspace identity")
}

pub(super) fn contains_recovery_file(root: &Path, marker: &str, contents: &[u8]) -> bool {
    fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_name().to_string_lossy().contains(marker)
                && fs::read(entry.path()).is_ok_and(|bytes| bytes == contents)
        })
}
