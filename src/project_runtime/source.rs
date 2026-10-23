use std::path::Path;

/// Confirms that a task checkout is exactly the reviewed commit with no
/// tracked or untracked changes that could alter a recipe or QA observation.
pub(crate) fn verify_source_checkout(
    repo: &Path,
    expected_sha: &str,
    allowed_untracked_outputs: &[String],
) -> Result<(), String> {
    if !matches!(expected_sha.len(), 40 | 64)
        || !expected_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("the reviewed source revision is not a valid Git object ID".into());
    }
    let mut command = crate::core::planning_sync::validated_git_command(repo)
        .map_err(|error| format!("task checkout Git configuration is unsafe: {error}"))?;
    let output = command
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .map_err(|error| format!("task checkout revision could not be read: {error}"))?;
    if !output.status.success() {
        return Err("task checkout has no readable HEAD commit".into());
    }
    let actual_sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if actual_sha != expected_sha {
        return Err("task checkout HEAD no longer matches the reviewed source revision".into());
    }

    let mut command = crate::core::planning_sync::validated_git_command(repo)
        .map_err(|error| format!("task checkout Git configuration is unsafe: {error}"))?;
    let output = command
        .args([
            "-c",
            "core.filemode=true",
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "-z",
        ])
        .output()
        .map_err(|error| format!("task checkout changes could not be inspected: {error}"))?;
    if !output.status.success() {
        return Err("task checkout changes could not be inspected".into());
    }
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        if entry.starts_with(b"?? ") {
            let path = String::from_utf8_lossy(&entry[3..]);
            let is_approved_output = allowed_untracked_outputs
                .iter()
                .filter_map(|allowed| normalize_output_path(allowed))
                .any(|allowed| output_path_covers(&allowed, Path::new(path.as_ref())));
            if is_approved_output {
                continue;
            }
        }
        return Err("task checkout has unreviewed tracked or unapproved untracked changes".into());
    }
    let mut command = crate::core::planning_sync::validated_git_command(repo)
        .map_err(|error| format!("task checkout Git configuration is unsafe: {error}"))?;
    let ignored = command
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| format!("ignored task checkout files could not be inspected: {error}"))?;
    if !ignored.status.success() {
        return Err("ignored task checkout files could not be inspected".into());
    }
    for path in ignored
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = String::from_utf8_lossy(path);
        if !allowed_untracked_outputs
            .iter()
            .filter_map(|allowed| normalize_output_path(allowed))
            .any(|allowed| output_path_covers(&allowed, Path::new(path.as_ref())))
        {
            return Err("task checkout contains ignored files outside approved outputs".into());
        }
    }
    Ok(())
}

fn normalize_output_path(value: &str) -> Option<std::path::PathBuf> {
    let mut normalized = std::path::PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) if part != ".git" => normalized.push(part),
            _ => return None,
        }
    }
    if normalized.as_os_str().is_empty() {
        Some(std::path::PathBuf::from("."))
    } else {
        Some(normalized)
    }
}

fn output_path_covers(allowed: &Path, candidate: &Path) -> bool {
    allowed == Path::new(".") || candidate == allowed || candidate.starts_with(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn verifies_exact_clean_reviewed_checkout() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        git(repo.path(), &["config", "user.name", "Test User"]);
        git(
            repo.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        fs::write(repo.path().join("source.txt"), "reviewed\n").unwrap();
        fs::write(repo.path().join(".gitignore"), "ignored/\n").unwrap();
        git(repo.path(), &["add", "source.txt"]);
        git(repo.path(), &["add", ".gitignore"]);
        git(repo.path(), &["commit", "--quiet", "-m", "reviewed source"]);
        let head = git(repo.path(), &["rev-parse", "HEAD"]);

        verify_source_checkout(repo.path(), &head, &[]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            git(repo.path(), &["config", "core.filemode", "false"]);
            let mut permissions = fs::metadata(repo.path().join("source.txt"))
                .unwrap()
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(repo.path().join("source.txt"), permissions).unwrap();
            assert!(verify_source_checkout(repo.path(), &head, &[]).is_err());
            let mut permissions = fs::metadata(repo.path().join("source.txt"))
                .unwrap()
                .permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(repo.path().join("source.txt"), permissions).unwrap();
        }
        fs::write(repo.path().join("source.txt"), "changed\n").unwrap();
        assert!(verify_source_checkout(repo.path(), &head, &[]).is_err());
        fs::write(repo.path().join("source.txt"), "reviewed\n").unwrap();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(repo.path().join("dist/index.html"), "built artifact\n").unwrap();
        verify_source_checkout(repo.path(), &head, &["dist".into()]).unwrap();
        verify_source_checkout(repo.path(), &head, &["./dist".into()]).unwrap();
        fs::write(repo.path().join("untracked.txt"), "unreviewed\n").unwrap();
        assert!(verify_source_checkout(repo.path(), &head, &["dist".into()]).is_err());
        fs::remove_file(repo.path().join("untracked.txt")).unwrap();
        fs::create_dir(repo.path().join("ignored")).unwrap();
        fs::write(
            repo.path().join("ignored/cache.txt"),
            "unreviewed ignored file\n",
        )
        .unwrap();
        assert!(verify_source_checkout(repo.path(), &head, &["dist".into()]).is_err());
        assert!(
            verify_source_checkout(repo.path(), &head, &["dist".into(), "ignored".into()]).is_ok()
        );
        assert!(verify_source_checkout(repo.path(), &"a".repeat(40), &[]).is_err());
    }
}
