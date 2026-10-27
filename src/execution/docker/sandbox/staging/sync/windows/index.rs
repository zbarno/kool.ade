use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::core::planning_sync::validated_git_command;

const MAX_INDEX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Entry {
    executable: Option<bool>,
    stage: u8,
    empty_object: bool,
}

pub(super) struct IndexModes {
    entries: BTreeMap<PathBuf, Vec<Entry>>,
}

impl IndexModes {
    pub(super) fn read(root: &Path) -> Result<Self, String> {
        let mut command =
            validated_git_command(root).map_err(|_| "Task-clone Git configuration is unsafe")?;
        let mut child = command
            .args([
                "-c",
                "core.fsmonitor=false",
                "--literal-pathspecs",
                "ls-files",
                "--stage",
                "-z",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Task-clone Git index could not be inspected")?;
        let (bytes, status) =
            crate::execution::docker::sandbox::staging::bounded_child::read_child_stdout_bounded(
                &mut child,
                MAX_INDEX_OUTPUT_BYTES,
            )
            .map_err(|_| "Task-clone Git index exceeded its safety limit or could not be read")?;
        if !status.success() {
            return Err("Task-clone Git index could not be inspected safely".into());
        }
        parse_index(&bytes)
    }

    pub(super) fn executable(&self, path: &Path) -> Result<Option<bool>, String> {
        let Some(entries) = self.entries.get(path) else {
            return Ok(None);
        };
        if entries.len() != 1 {
            return Err("Task-clone Git index has unresolved file stages".into());
        }
        let entry = entries[0];
        match (entry.stage, entry.executable, entry.empty_object) {
            (0, Some(executable), false) => Ok(Some(executable)),
            _ => Err("Task-clone Git index mode is unsupported or incomplete".into()),
        }
    }

    pub(super) fn ensure_current(
        root: &Path,
        path: &Path,
        expected: Option<bool>,
    ) -> Result<(), String> {
        if Self::read(root)?.executable(path)? == expected {
            Ok(())
        } else {
            Err("Task-clone Git index mode changed during synchronization".into())
        }
    }

    pub(super) fn set_if_current(
        root: &Path,
        path: &Path,
        expected: bool,
        target: bool,
    ) -> Result<(), String> {
        if Self::read(root)?.executable(path)? != Some(expected) {
            return Err("Task-clone Git index mode changed during synchronization".into());
        }
        set_executable(root, path, target)?;
        if Self::read(root)?.executable(path)? != Some(target) {
            return Err("Task-clone Git index mode changed during synchronization".into());
        }
        Ok(())
    }

    pub(super) fn restore_if_current(
        root: &Path,
        path: &Path,
        expected: bool,
        original: bool,
    ) -> Result<(), String> {
        let current = Self::read(root)?.executable(path)?;
        if current == Some(original) {
            return Ok(());
        }
        if current != Some(expected) {
            return Err("Git index mode changed concurrently and was preserved".into());
        }
        set_executable(root, path, original)?;
        if Self::read(root)?.executable(path)? != Some(original) {
            return Err("Task-clone Git index mode rollback could not be verified".into());
        }
        Ok(())
    }
}

pub(super) fn set_executable(root: &Path, path: &Path, executable: bool) -> Result<(), String> {
    let path = path
        .to_str()
        .ok_or("Workspace output path is not valid UTF-8")?;
    let pathspec = path.replace('\\', "/");
    let chmod = if executable {
        "--chmod=+x"
    } else {
        "--chmod=-x"
    };
    let mut command =
        validated_git_command(root).map_err(|_| "Task-clone Git configuration is unsafe")?;
    let status = command
        .args([
            "-c",
            "core.fsmonitor=false",
            "--literal-pathspecs",
            "update-index",
            chmod,
            "--",
            pathspec.as_str(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "Task-clone Git index mode could not be updated")?;
    if !status.success() {
        return Err("Task-clone Git index mode could not be updated".into());
    }
    Ok(())
}

fn parse_index(bytes: &[u8]) -> Result<IndexModes, String> {
    let mut entries = BTreeMap::<PathBuf, Vec<Entry>>::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or("Task-clone Git index data is malformed")?;
        let metadata = &record[..separator];
        let path = &record[separator + 1..];
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 || path.is_empty() {
            return Err("Task-clone Git index data is malformed".into());
        }
        let executable = match fields[0] {
            b"100644" => Some(false),
            b"100755" => Some(true),
            b"120000" | b"160000" => None,
            _ => return Err("Task-clone Git index mode is malformed".into()),
        };
        let object = fields[1];
        if !matches!(object.len(), 40 | 64) || !object.iter().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Task-clone Git index object identity is malformed".into());
        }
        let stage = std::str::from_utf8(fields[2])
            .ok()
            .and_then(|stage| stage.parse::<u8>().ok())
            .filter(|stage| *stage <= 3)
            .ok_or("Task-clone Git index stage is malformed")?;
        let Ok(path) = std::str::from_utf8(path) else {
            continue;
        };
        entries
            .entry(PathBuf::from(path.replace('/', "\\")))
            .or_default()
            .push(Entry {
                executable,
                stage,
                empty_object: object.iter().all(|byte| *byte == b'0'),
            });
    }
    Ok(IndexModes { entries })
}
