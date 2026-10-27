use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, ensure};

const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct Snapshot {
    raw: Vec<u8>,
    modes: BTreeMap<String, bool>,
    paths: Vec<String>,
}

impl Snapshot {
    pub(super) fn read(root: &Path) -> anyhow::Result<Self> {
        let mut child = crate::core::planning_sync::validated_git_command(root)?
            .args(["ls-files", "--stage", "-z"])
            .stdout(Stdio::piped())
            .spawn()
            .context("workspace Git index could not be inspected")?;
        let (raw, status) =
            crate::execution::docker::sandbox::staging::bounded_child::read_child_stdout_bounded(
                &mut child,
                MAX_INDEX_BYTES,
            )
            .context("workspace Git index output exceeded its safety limit or could not be read")?;
        ensure!(
            status.success(),
            "workspace Git index could not be inspected"
        );
        Self::parse(raw)
    }

    fn parse(raw: Vec<u8>) -> anyhow::Result<Self> {
        let mut modes = BTreeMap::new();
        let mut paths = Vec::new();
        ensure!(
            raw.is_empty() || raw.last() == Some(&0),
            "workspace Git index listing is malformed"
        );
        let records = if raw.is_empty() {
            Vec::new()
        } else {
            raw[..raw.len() - 1]
                .split(|byte| *byte == 0)
                .collect::<Vec<_>>()
        };
        for record in records {
            ensure!(
                !record.is_empty(),
                "workspace Git index record is malformed"
            );
            let separator = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| anyhow::anyhow!("workspace Git index record is malformed"))?;
            let fields = record[..separator]
                .split(|byte| *byte == b' ')
                .collect::<Vec<_>>();
            ensure!(
                fields.len() == 3
                    && matches!(fields[1].len(), 40 | 64)
                    && fields[1].iter().all(u8::is_ascii_hexdigit),
                "workspace Git index record is malformed"
            );
            let executable = match fields[0] {
                b"100644" => false,
                b"100755" => true,
                _ => anyhow::bail!("workspace Git index contains an unsupported file type"),
            };
            ensure!(
                fields[2] == b"0",
                "workspace Git index contains unresolved conflicts"
            );
            let path = std::str::from_utf8(&record[separator + 1..])
                .context("workspace Git index path is not valid UTF-8")?;
            ensure!(
                !path.contains('\\'),
                "workspace Git index path uses an unsupported separator"
            );
            let parsed_path = Path::new(path);
            super::super::super::validate_relative(parsed_path)?;
            let path = path.to_owned();
            ensure!(
                modes.insert(path.clone(), executable).is_none(),
                "workspace Git index contains duplicate paths"
            );
            paths.push(path);
        }
        validate_windows_paths(&paths)?;
        Ok(Self { raw, modes, paths })
    }

    pub(super) fn executable_for(&self, relative: &Path, scope: Option<&Path>) -> bool {
        let key = index_path(relative, scope);
        self.modes.get(&key).copied().unwrap_or(false)
    }

    pub(super) fn validate_captured_paths(
        &self,
        captured_paths: &[String],
        scope: Option<&Path>,
    ) -> anyhow::Result<()> {
        let mut all_paths = self.paths.clone();
        if let Some(scope) = scope.filter(|path| *path != Path::new(".")) {
            all_paths.push(scope.to_string_lossy().replace('\\', "/"));
        }
        all_paths.extend(
            captured_paths
                .iter()
                .map(|path| index_path(Path::new(path), scope)),
        );
        validate_windows_paths(&all_paths)
    }

    pub(super) fn ensure_unchanged(&self, after: &Self) -> anyhow::Result<()> {
        ensure!(
            self.raw == after.raw,
            "workspace Git index changed during capture"
        );
        Ok(())
    }
}

fn index_path(relative: &Path, scope: Option<&Path>) -> String {
    let relative = relative.to_string_lossy().replace('\\', "/");
    let scope = scope
        .filter(|path| *path != Path::new("."))
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if scope.is_empty() {
        relative
    } else if relative.is_empty() {
        scope
    } else {
        format!("{scope}/{relative}")
    }
}

fn validate_windows_paths(paths: &[String]) -> anyhow::Result<()> {
    let paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
    super::super::super::windows_paths::validate_collisions(paths.iter().map(PathBuf::as_path))
}

#[cfg(test)]
#[path = "index/tests.rs"]
mod tests;
