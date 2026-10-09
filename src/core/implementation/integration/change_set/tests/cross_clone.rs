use super::*;

struct CloneFixture {
    root: TempRepo,
    task: PathBuf,
    peer: PathBuf,
    base: String,
}

impl CloneFixture {
    fn new() -> Self {
        let root = TempRepo::new();
        let remote = root.0.join("remote.git");
        let seed = root.0.join("seed");
        fs::create_dir(&seed).unwrap();
        git(&root.0, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(&seed, &["init", "-q", "-b", "main"]);
        git(&seed, &["config", "user.name", "Fixture"]);
        git(&seed, &["config", "user.email", "fixture@example.test"]);
        for name in ["rename-old.txt", "deleted.txt", "binary.dat", "target-a"] {
            fs::write(seed.join(name), name.as_bytes()).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink("target-a", seed.join("link")).unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "base"]);
        git(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&seed, &["push", "-q", "-u", "origin", "main"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        let task = root.0.join("task-a");
        let peer = root.0.join("task-b");
        for (path, branch) in [(&task, "task-a-uid"), (&peer, "task-b-uid")] {
            git(
                &root.0,
                &[
                    "clone",
                    "-q",
                    remote.to_str().unwrap(),
                    path.to_str().unwrap(),
                ],
            );
            git(path, &["config", "user.name", "Fixture"]);
            git(path, &["config", "user.email", "fixture@example.test"]);
            git(path, &["switch", "-q", "-c", branch]);
        }
        let base = git(&task, &["rev-parse", "HEAD"]);
        Self {
            root,
            task,
            peer,
            base,
        }
    }

    fn task_commit(&self, name: &str) -> String {
        git(&self.task, &["add", "-A"]);
        git(&self.task, &["commit", "-qm", name]);
        git(&self.task, &["rev-parse", "HEAD"])
    }

    fn integrate_peer(&self) -> String {
        git(&self.peer, &["add", "-A"]);
        git(&self.peer, &["commit", "-qm", "task B result"]);
        git(&self.peer, &["switch", "-q", "main"]);
        git(&self.peer, &["merge", "--squash", "task-b-uid"]);
        git(&self.peer, &["commit", "-qm", "integrate task B"]);
        git(&self.peer, &["push", "-q", "origin", "main"]);
        git(&self.task, &["fetch", "-q", "origin"]);
        git(&self.task, &["rev-parse", "origin/main"])
    }

    fn conflicts(&self, task_head: &str, destination_head: &str) -> Vec<PathBuf> {
        overlap(
            &self.task,
            &self.base,
            task_head,
            destination_head,
            &self.root.0.join("audit"),
            &fixture_runner(),
        )
        .unwrap()
    }
}

#[cfg(unix)]
#[test]
fn distinct_task_clones_detect_rename_delete_binary_and_symlink_conflicts() {
    let fixture = CloneFixture::new();
    fs::rename(
        fixture.task.join("rename-old.txt"),
        fixture.task.join("rename-new.txt"),
    )
    .unwrap();
    fs::remove_file(fixture.task.join("deleted.txt")).unwrap();
    fs::write(fixture.task.join("binary.dat"), [0, 1, 2, 255]).unwrap();
    fs::remove_file(fixture.task.join("link")).unwrap();
    std::os::unix::fs::symlink("target-b", fixture.task.join("link")).unwrap();
    let task_head = fixture.task_commit("task A result");
    let task_changes = collect(
        &fixture.task,
        &fixture.base,
        &task_head,
        &fixture.root.0.join("task-changes.z"),
        &fixture_runner(),
    )
    .unwrap();
    assert!(
        task_changes
            .paths
            .contains(&PathBuf::from("rename-old.txt"))
    );
    assert!(
        task_changes
            .paths
            .contains(&PathBuf::from("rename-new.txt"))
    );

    fs::write(fixture.peer.join("rename-old.txt"), "peer edits old name\n").unwrap();
    fs::write(
        fixture.peer.join("deleted.txt"),
        "peer edits deleted path\n",
    )
    .unwrap();
    fs::write(fixture.peer.join("binary.dat"), [9, 8, 7, 255]).unwrap();
    fs::remove_file(fixture.peer.join("link")).unwrap();
    std::os::unix::fs::symlink("target-c", fixture.peer.join("link")).unwrap();
    let destination_head = fixture.integrate_peer();

    let conflicts = fixture.conflicts(&task_head, &destination_head);
    for path in ["rename-old.txt", "deleted.txt", "binary.dat", "link"] {
        assert!(
            conflicts.contains(&PathBuf::from(path)),
            "{path}: {conflicts:?}"
        );
    }
    assert_eq!(
        git(&fixture.task, &["branch", "--show-current"]),
        "task-a-uid"
    );
    assert_eq!(
        fs::read(fixture.task.join("binary.dat")).unwrap(),
        [0, 1, 2, 255]
    );
}

#[test]
fn distinct_task_clones_keep_disjoint_git_changes_integrable() {
    let fixture = CloneFixture::new();
    fs::write(fixture.task.join("task-a.txt"), "task A\n").unwrap();
    let task_head = fixture.task_commit("task A result");
    fs::write(fixture.peer.join("task-b.txt"), "task B\n").unwrap();
    let destination_head = fixture.integrate_peer();
    assert!(fixture.conflicts(&task_head, &destination_head).is_empty());
}
