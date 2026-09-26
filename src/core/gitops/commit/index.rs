//! Preserve the user's staged entries around Packet's path-limited commit.
use super::command_error;
use crate::core::gitops::{require_exit_success, run, run_with_input};
use crate::error::AppError;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Default)]
pub(super) struct IndexSnapshot {
    entries: BTreeMap<String, Vec<String>>,
}

pub(super) fn snapshot(cwd: &Path, specs: &[String]) -> Result<IndexSnapshot, AppError> {
    let mut args = vec![
        "ls-files".to_owned(),
        "--stage".into(),
        "-z".into(),
        "--".into(),
    ];
    args.extend(specs.iter().cloned());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let (code, output, error) = run(cwd, &refs)?;
    require_exit_success("ls-files --stage", code, output.clone(), error)?;

    let mut snapshot = IndexSnapshot::default();
    for record in output
        .as_bytes()
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err(command_error(
                "ls-files --stage",
                "git returned a malformed index entry",
            ));
        };
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|error| command_error("ls-files --stage", &error.to_string()))?;
        let path = std::str::from_utf8(&record[tab + 1..])
            .map_err(|error| command_error("ls-files --stage", &error.to_string()))?;
        if header.split_whitespace().count() != 3 {
            return Err(command_error(
                "ls-files --stage",
                "git returned a malformed index entry",
            ));
        }
        snapshot
            .entries
            .entry(path.to_owned())
            .or_default()
            .push(header.to_owned());
    }
    Ok(snapshot)
}

pub(super) fn staged_paths(
    cwd: &Path,
    paths: &[String],
    specs: &[String],
) -> Result<BTreeSet<String>, AppError> {
    let mut staged = BTreeSet::new();
    for (path, spec) in paths.iter().zip(specs) {
        let (code, stdout, stderr) = run(cwd, &["diff", "--cached", "--quiet", "--", spec])?;
        match code {
            0 => {}
            1 => {
                staged.insert(path.clone());
            }
            _ => return Err(command_error("diff --cached", &format!("{stdout}{stderr}"))),
        }
    }
    Ok(staged)
}

pub(super) fn reject_unmerged_paths(cwd: &Path, specs: &[String]) -> Result<(), AppError> {
    let mut args = vec![
        "ls-files".to_owned(),
        "--unmerged".into(),
        "-z".into(),
        "--".into(),
    ];
    args.extend(specs.iter().cloned());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let (code, output, error) = run(cwd, &refs)?;
    require_exit_success("ls-files --unmerged", code, output.clone(), error)?;
    if !output.is_empty() {
        return Err(command_error(
            "commit",
            "an authorized path has unresolved user merge stages; resolve it before Packet checkpoints this file",
        ));
    }
    Ok(())
}

/// Restore original index entries for selected paths. This keeps unrelated
/// staged work and overlapping user edits logically intact after HEAD moves.
pub(super) fn restore_paths(
    cwd: &Path,
    snapshot: &IndexSnapshot,
    paths: &BTreeSet<String>,
) -> Result<(), AppError> {
    if paths.is_empty() {
        return Ok(());
    }
    let (code, format, error) = run(cwd, &["rev-parse", "--show-object-format"])?;
    require_exit_success(
        "rev-parse --show-object-format",
        code,
        format.clone(),
        error,
    )?;
    let hash_len = match format.trim() {
        "sha1" => 40,
        "sha256" => 64,
        other => {
            return Err(command_error(
                "restore index",
                &format!("unsupported Git object format: {other}"),
            ));
        }
    };
    let zeros = "0".repeat(hash_len);
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(format!("0 {zeros} 0\t{path}\0").as_bytes());
        if let Some(entries) = snapshot.entries.get(path) {
            for entry in entries {
                input.extend_from_slice(format!("{entry}\t{path}\0").as_bytes());
            }
        }
    }
    let (code, stdout, stderr) =
        run_with_input(cwd, &["update-index", "-z", "--index-info"], &input)?;
    require_exit_success("update-index --index-info", code, stdout, stderr)
}

pub(super) fn preserve_or_report(
    cwd: &Path,
    snapshot: &IndexSnapshot,
    paths: &BTreeSet<String>,
    original: AppError,
) -> AppError {
    match restore_paths(cwd, snapshot, paths) {
        Ok(()) => original,
        Err(restore) => command_error(
            "preserve user's staged changes",
            &format!(
                "{original}; additionally, restoring the original index entries failed: {restore}"
            ),
        ),
    }
}
