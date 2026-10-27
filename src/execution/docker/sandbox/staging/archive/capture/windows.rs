use std::{fs::File, path::Path};

use anyhow::{Context, ensure};
use tar::Builder;

use super::super::super::super::workspace_identity::{self, WorkspaceObjectId};
use super::super::super::WorkspaceSnapshot;

mod entries;
mod enumeration;
mod format;
mod index;
mod metadata;
mod native;

pub(super) fn capture(
    root: &Path,
    expected_root_identity: &WorkspaceObjectId,
    scope: Option<&Path>,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
) -> anyhow::Result<WorkspaceSnapshot> {
    capture_excluding(
        root,
        expected_root_identity,
        scope,
        byte_limit,
        read_only,
        entry_limit,
        &[],
    )
}

pub(super) fn capture_excluding(
    root_path: &Path,
    expected_root_identity: &WorkspaceObjectId,
    scope: Option<&Path>,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
    excluded_paths: &[String],
) -> anyhow::Result<WorkspaceSnapshot> {
    let root = native::open_root(root_path)?;
    metadata::verify_identity(&root.directory)?;
    let actual_identity = workspace_identity::from_handle(&root.directory)
        .ok_or_else(|| anyhow::anyhow!("workspace root identity could not be verified"))?;
    ensure!(
        &actual_identity == expected_root_identity,
        "workspace root changed before capture"
    );
    let index = index::Snapshot::read(root_path)?;
    let scope_path = scope;
    let scoped_directory = match scope_path {
        Some(path) => Some(native::open_scope(&root.directory, path)?),
        None => None,
    };
    let directory = scoped_directory
        .as_ref()
        .map_or(&root.directory, |scoped| &scoped.directory);
    let mut builder = Builder::new(Vec::new());
    let mut manifest = super::super::Manifest::new();
    let mut total_bytes = 0_u64;
    let mut captured_paths = Vec::new();
    entries::append_directory(
        directory,
        Path::new(""),
        &mut builder,
        &mut manifest,
        &mut total_bytes,
        byte_limit,
        entry_limit,
        read_only,
        excluded_paths,
        &index,
        scope_path,
        &mut captured_paths,
    )?;
    index.validate_captured_paths(&captured_paths, scope_path)?;
    index.ensure_unchanged(&index::Snapshot::read(root_path)?)?;
    let archive = builder
        .into_inner()
        .context("workspace archive could not be finalized")?;
    ensure!(
        archive.len() as u64 <= byte_limit,
        "archive exceeds staging budget"
    );
    Ok(WorkspaceSnapshot {
        archive,
        manifest,
        scope_root: None,
    })
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;
