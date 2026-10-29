use std::{collections::BTreeMap, path::PathBuf};

use super::{WorkspaceSnapshot, archive, capture, capture_scoped, failure};
use crate::{
    execution::{docker::sandbox::ValidatedWorkspace, errors::ExecutionError},
    project_runtime::verify_source_checkout_scoped,
};

pub(crate) fn capture_reviewed(
    workspace: &ValidatedWorkspace,
    expected_sha: &str,
    allowed_source_inputs: &[String],
    scope: Option<&str>,
    budget: u64,
    read_only: bool,
) -> Result<WorkspaceSnapshot, ExecutionError> {
    verify_reviewed_checkout(workspace, expected_sha, allowed_source_inputs, scope)?;
    let snapshot = if let Some(root_relative) = scope {
        capture_scoped(workspace, root_relative, budget)?
    } else {
        capture(workspace, budget, read_only)?
    };
    verify_snapshot_matches_git_tree(
        &snapshot,
        workspace.root(),
        expected_sha,
        allowed_source_inputs,
    )?;
    verify_reviewed_checkout(workspace, expected_sha, allowed_source_inputs, scope)?;
    Ok(snapshot)
}

fn verify_reviewed_checkout(
    workspace: &ValidatedWorkspace,
    expected_sha: &str,
    allowed_source_inputs: &[String],
    scope: Option<&str>,
) -> Result<(), ExecutionError> {
    workspace.revalidate()?;
    verify_source_checkout_scoped(workspace.root(), expected_sha, allowed_source_inputs, scope)
        .map_err(|_| failure("Reviewed task source changed before execution staging"))
}

pub(crate) fn verify_snapshot_matches_git_tree(
    snapshot: &WorkspaceSnapshot,
    root: &std::path::Path,
    expected_sha: &str,
    allowed_source_inputs: &[String],
) -> Result<(), ExecutionError> {
    let mut command = crate::core::planning_sync::validated_git_command(root)
        .map_err(|_| failure("Reviewed task source Git configuration is unsafe"))?;
    let output = command
        .args(["ls-tree", "-r", "-z", "--full-tree", expected_sha])
        .output()
        .map_err(|_| failure("Reviewed task source tree could not be inspected"))?;
    if !output.status.success() {
        return Err(failure("Reviewed task source tree could not be inspected"));
    }
    let mut blobs = BTreeMap::<PathBuf, (String, bool)>::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        let Some(separator) = record.iter().position(|byte| *byte == b'\t') else {
            return Err(failure("Reviewed task source tree is malformed"));
        };
        let (metadata, path) = record.split_at(separator);
        let fields = metadata.split(|byte| *byte == b' ').collect::<Vec<_>>();
        if fields.len() != 3 || fields[1] != b"blob" {
            continue;
        }
        let path = std::str::from_utf8(&path[1..])
            .map_err(|_| failure("Reviewed task source path is not UTF-8"))?;
        blobs.insert(
            PathBuf::from(path),
            (
                String::from_utf8_lossy(fields[2]).into_owned(),
                fields[0] == b"100755",
            ),
        );
    }

    let scope = snapshot.scope_root.as_deref();
    for path in blobs.keys().filter(|path| {
        scope.is_none_or(|root| root == std::path::Path::new(".") || path.starts_with(root))
    }) {
        if !matches!(
            snapshot.manifest.get(path),
            Some(archive::Fingerprint::File { .. })
        ) {
            return Err(failure("Staged snapshot is missing a reviewed Git file"));
        }
    }

    let mut tracked_files = Vec::new();
    for (path, fingerprint) in &snapshot.manifest {
        let archive::Fingerprint::File { sha256, size, mode } = fingerprint else {
            continue;
        };
        if let Some((object_id, executable)) = blobs.get(path) {
            if (*mode & 0o111 != 0) != *executable {
                return Err(failure(
                    "Staged file mode does not match the reviewed Git tree",
                ));
            }
            tracked_files.push((object_id.clone(), sha256.clone(), *size));
        } else if !allowed_source_inputs.iter().any(|allowed| {
            allowed == "." || path == std::path::Path::new(allowed) || path.starts_with(allowed)
        }) {
            return Err(failure("Staged file is outside approved recipe outputs"));
        }
    }
    if !tracked_files.is_empty() {
        use std::io::Write;
        let mut command = crate::core::planning_sync::validated_git_command(root)
            .map_err(|_| failure("Reviewed task source Git configuration is unsafe"))?;
        let mut child = command
            .args(["cat-file", "--batch"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| failure("Reviewed task source files could not be verified"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| failure("Reviewed task source verifier could not start"))?;
        let expected = tracked_files.clone();
        let reader = std::thread::spawn(move || verify_blob_stream(stdout, &expected));
        let write_result = {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| failure("Reviewed task source verifier could not start"))?;
            tracked_files
                .iter()
                .try_for_each(|(object_id, _, _)| writeln!(stdin, "{object_id}"))
        };
        drop(child.stdin.take());
        let status = child
            .wait()
            .map_err(|_| failure("Reviewed task source verifier did not finish"))?;
        let verified = reader
            .join()
            .map_err(|_| failure("Reviewed task source verifier stopped unexpectedly"))?;
        if write_result.is_err() || !status.success() || verified.is_err() {
            return Err(failure("Reviewed task source files could not be verified"));
        }
    }
    Ok(())
}

fn verify_blob_stream(
    stdout: impl std::io::Read,
    expected: &[(String, String, u64)],
) -> Result<(), &'static str> {
    use sha2::Digest;
    use std::io::{BufRead, BufReader, Read};

    let mut reader = BufReader::new(stdout);
    let mut buffer = [0_u8; 64 * 1024];
    for (object_id, expected_sha256, expected_size) in expected {
        let mut header = Vec::new();
        if reader
            .read_until(b'\n', &mut header)
            .map_err(|_| "read failed")?
            == 0
        {
            return Err("missing object header");
        }
        if header.last() != Some(&b'\n') {
            return Err("incomplete object header");
        }
        let fields = std::str::from_utf8(&header[..header.len() - 1])
            .map_err(|_| "invalid object header")?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 3 || fields[0] != object_id || fields[1] != "blob" {
            return Err("unexpected object response");
        }
        let size = fields[2]
            .parse::<u64>()
            .map_err(|_| "invalid object size")?;
        if size != *expected_size {
            return Err("object size differs from the staged file");
        }
        let mut remaining = size;
        let mut hasher = sha2::Sha256::new();
        while remaining > 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            reader
                .read_exact(&mut buffer[..count])
                .map_err(|_| "incomplete object content")?;
            hasher.update(&buffer[..count]);
            remaining -= count as u64;
        }
        let mut newline = [0_u8; 1];
        reader
            .read_exact(&mut newline)
            .map_err(|_| "missing object delimiter")?;
        let digest = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if newline[0] != b'\n' || digest != *expected_sha256 {
            return Err("object content differs from staged file");
        }
    }
    let mut trailing = [0_u8; 1];
    if reader.read(&mut trailing).map_err(|_| "read failed")? != 0 {
        return Err("unexpected trailing object data");
    }
    Ok(())
}
