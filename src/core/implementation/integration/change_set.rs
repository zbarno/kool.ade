use super::*;
use std::{collections::BTreeSet, ffi::OsString};

#[derive(Debug, Default)]
struct ChangeSet {
    paths: BTreeSet<PathBuf>,
}

pub(super) fn overlap(
    repo: &Path,
    task_base: &str,
    task_head: &str,
    destination_head: &str,
    audit_dir: &Path,
    runner: &Runner,
) -> anyhow::Result<Vec<PathBuf>> {
    let task_merge_base = runner.merge_base(repo, task_base, task_head)?;
    anyhow::ensure!(
        task_merge_base.as_deref() == Some(task_base),
        "The verified task commit no longer descends from its recorded base; its clone is preserved for review"
    );
    let destination_merge_base = runner
        .merge_base(repo, task_base, destination_head)?
        .ok_or_else(|| {
            crate::core::implementation::initial_reconciliation::support::user_action(
                "Task and destination histories have no common commit. Both histories are preserved; review them before integration.".into(),
            )
        })?;
    fs::create_dir_all(audit_dir)?;
    let task = collect(
        repo,
        task_base,
        task_head,
        &audit_dir.join("task.z"),
        runner,
    )?;
    let destination = collect(
        repo,
        &destination_merge_base,
        destination_head,
        &audit_dir.join("destination.z"),
        runner,
    )?;
    Ok(overlaps(&task, &destination))
}

fn collect(
    repo: &Path,
    base: &str,
    head: &str,
    output: &Path,
    runner: &Runner,
) -> anyhow::Result<ChangeSet> {
    runner.git_to_file(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--find-copies",
            base,
            head,
        ],
        output,
    )?;
    parse_name_status(&fs::read(output)?)
}

fn parse_name_status(data: &[u8]) -> anyhow::Result<ChangeSet> {
    let fields = data.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut changes = ChangeSet::default();
    let mut index = 0;
    while index < fields.len() {
        let status = fields[index];
        index += 1;
        if status.is_empty() {
            continue;
        }
        let code = status[0];
        let path_count = if matches!(code, b'R' | b'C') { 2 } else { 1 };
        anyhow::ensure!(
            matches!(
                code,
                b'A' | b'C' | b'D' | b'M' | b'R' | b'T' | b'U' | b'X' | b'B'
            ),
            "Git returned an unsupported changed-path status"
        );
        anyhow::ensure!(
            index + path_count <= fields.len(),
            "Git returned an incomplete changed-path record"
        );
        for path in &fields[index..index + path_count] {
            changes.paths.insert(normalize_path(path)?);
        }
        index += path_count;
    }
    Ok(changes)
}

fn normalize_path(path: &[u8]) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(!path.is_empty(), "Git returned an empty changed path");
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(path.to_vec()))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(std::str::from_utf8(path)?);
    anyhow::ensure!(
        !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
        "Git returned a changed path outside the repository"
    );
    Ok(path)
}

fn overlaps(task: &ChangeSet, destination: &ChangeSet) -> Vec<PathBuf> {
    let mut conflicts = BTreeSet::new();
    for path in &task.paths {
        for ancestor in path.ancestors() {
            if destination.paths.contains(ancestor) {
                conflicts.insert(path.clone());
                conflicts.insert(ancestor.to_path_buf());
            }
        }
    }
    for path in &destination.paths {
        for ancestor in path.ancestors() {
            if task.paths.contains(ancestor) {
                conflicts.insert(path.clone());
                conflicts.insert(ancestor.to_path_buf());
            }
        }
    }
    conflicts.into_iter().collect()
}

pub(super) fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| format!("{path:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    #[path = "cross_clone.rs"]
    mod cross_clone;

    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::{Arc, atomic::AtomicBool, mpsc},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("koolade-change-set-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn fixture_runner() -> Runner {
        let (progress, _updates) = mpsc::channel();
        Runner {
            gh: "gh".into(),
            runtime_config_source: None,
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        }
    }

    #[test]
    fn parses_rename_delete_typechange_and_special_path_names_from_nul_records() {
        let output = b"R100\0old\tname\0new\nname\0D\0removed\0T\0kind-change\0M\0binary\xff\0";
        let changes = parse_name_status(output).unwrap();
        assert_eq!(changes.paths.len(), 5);
        assert!(changes.paths.contains(Path::new("old\tname")));
        assert!(changes.paths.contains(Path::new("new\nname")));
        assert!(changes.paths.contains(Path::new("removed")));
        assert!(changes.paths.contains(Path::new("kind-change")));
    }

    #[test]
    fn detects_same_path_and_file_directory_prefix_conflicts() {
        let task = ChangeSet {
            paths: BTreeSet::from([PathBuf::from("Cargo.lock"), PathBuf::from("src/a")]),
        };
        let destination = ChangeSet {
            paths: BTreeSet::from([PathBuf::from("Cargo.lock"), PathBuf::from("src")]),
        };
        assert_eq!(
            overlaps(&task, &destination),
            [
                PathBuf::from("Cargo.lock"),
                PathBuf::from("src"),
                PathBuf::from("src/a")
            ]
        );
    }

    #[test]
    fn actual_git_commits_detect_same_path_and_file_directory_conflicts() {
        let root = TempRepo::new();
        let repo = root.0.as_path();
        git(repo, &["init", "-q", "-b", "main"]);
        git(repo, &["config", "user.name", "Fixture"]);
        git(repo, &["config", "user.email", "fixture@example.test"]);
        fs::write(repo.join("shared.txt"), "base\n").unwrap();
        git(repo, &["add", "shared.txt"]);
        git(repo, &["commit", "-qm", "base"]);
        let base = git(repo, &["rev-parse", "HEAD"]);

        fs::write(repo.join("shared.txt"), "task change\n").unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/child.txt"), "task file\n").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", "task change"]);
        let task_head = git(repo, &["rev-parse", "HEAD"]);

        git(repo, &["reset", "--hard", &base]);
        fs::write(repo.join("shared.txt"), "destination change\n").unwrap();
        fs::write(repo.join("src"), "destination file\n").unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-qm", "destination change"]);
        let destination_head = git(repo, &["rev-parse", "HEAD"]);

        let conflicts = overlap(
            repo,
            &base,
            &task_head,
            &destination_head,
            &root.0.join("audit"),
            &fixture_runner(),
        )
        .unwrap();
        assert!(conflicts.contains(&PathBuf::from("shared.txt")));
        assert!(conflicts.contains(&PathBuf::from("src")));
        assert!(conflicts.contains(&PathBuf::from("src/child.txt")));
    }
}
