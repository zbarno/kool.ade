use super::*;

pub(super) fn create(initial_files: &[(&str, &str)]) -> Sandbox {
    let environment_lock = crate::test_support::lock_process_environment();
    let verification_executor = install_verification_executor();
    let root = std::env::temp_dir().join(format!(
        "koolade-implementation-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().hyphenated()
    ));
    fs::create_dir_all(&root).unwrap();
    let koolade_home_restore = RestoreKooladeHome::capture();
    // SAFETY: this fixture holds the shared cross-process environment lock and restores the prior value.
    unsafe {
        std::env::set_var("KOOLADE_HOME", root.join("koolade-home"));
    }
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.test"]);
    let ticket =
        ".koolade-packet/planning/tasks/feature/001-implement-ticket-behavior.md".to_owned();
    fs::create_dir_all(repo.join(".koolade-packet/planning/tasks/feature")).unwrap();
    let ticket_text = "# Implement ticket behavior\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n";
    let identity = crate::domain::ArtifactIdentity::preserve_markdown(
        ticket_text,
        None,
        "TASK-001",
        "Implement ticket behavior",
    )
    .unwrap();
    fs::write(repo.join(&ticket), &identity).unwrap();
    fs::write(
        repo.join(".koolade-packet/planning/tasks/feature/specification.md"),
        "# F1: Implement ticket behavior\n\n- File contains implemented.\n",
    )
    .unwrap();
    for (relative_path, contents) in initial_files {
        let path = repo.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    git(&["add", "."]);
    git(&["commit", "-qm", "baseline"]);
    git(&[
        "init",
        "--bare",
        "-q",
        root.join("remote.git").to_str().unwrap(),
    ]);
    let ssh = root.join("ssh-fixture");
    fs::write(
            &ssh,
            "#!/bin/sh\nroot=$(dirname \"$0\")\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack \"$root/remote.git\" ;;\n  *git-receive-pack*) exec git-receive-pack \"$root/remote.git\" ;;\n  *) echo 'unsupported fixture SSH command' >&2; exit 2 ;;\nesac\n",
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let ssh_command_restore = RestoreSshCommand::capture();
    // SAFETY: this fixture holds the shared cross-process environment lock and restores the prior value.
    unsafe {
        std::env::set_var("GIT_SSH_COMMAND", &ssh);
    }
    git(&[
        "remote",
        "add",
        "origin",
        "ssh://git@github.com/fixture/repo.git",
    ]);
    git(&["push", "-q", "origin", "main"]);
    let gh = root.join("gh-fixture");
    fs::write(
        &gh,
        r##"#!/bin/sh
root=$(dirname "$0")
if [ -f "$root/offline" ]; then echo 'simulated GitHub unavailable' >&2; exit 1; fi
if [ "$1" = run ] && [ "$2" = list ]; then
 commit=''
 while [ "$#" -gt 0 ]; do if [ "$1" = --commit ]; then shift; commit=$1; fi; shift; done
 printf '[{"headSha":"%s","status":"completed","conclusion":"success","workflowName":"fixture","createdAt":"2099-01-01T00:00:00Z","url":"https://github.com/fixture/repo/actions/runs/1"}]\n' "$commit"
elif [ "$2" = list ]; then
 if [ -f "$root/pr-created" ]; then echo '[{"url":"https://github.com/fixture/repo/pull/1","state":"OPEN"}]'; else echo '[]'; fi
elif [ "$2" = view ]; then
 head=$(git --git-dir "$root/remote.git" for-each-ref --format='%(objectname)' refs/heads/koolade | head -n 1)
 base_branch=$(awk 'previous {print; exit} $0=="--base" {previous=1}' "$root/pr-args")
 base=$(git --git-dir "$root/remote.git" rev-parse "refs/heads/$base_branch")
 printf '{"url":"https://github.com/fixture/repo/pull/1","number":1,"state":"OPEN","headRefOid":"%s","baseRefOid":"%s","baseRefName":"%s"}\n' "$head" "$base" "$base_branch"
elif [ "$2" = create ]; then
 if [ -f "$root/advance-main-on-create" ]; then
  git --git-dir "$root/remote.git" update-ref refs/heads/main "$(cat "$root/advance-main-on-create")"
 fi
 printf '%s\n' "$@" > "$root/pr-args"
 echo created >> "$root/pr-created"
 echo 'https://github.com/fixture/repo/pull/1'
else
 printf '%s\n' "$@" > "$root/pr-args"
 echo created >> "$root/pr-created"
 echo 'https://github.com/fixture/repo/pull/1'
fi
"##,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o700)).unwrap();
    }
    Sandbox {
        root,
        repo,
        gh,
        ticket,
        latest_peer: std::sync::Mutex::new(None),
        _koolade_home_restore: koolade_home_restore,
        _ssh_command_restore: ssh_command_restore,
        _environment_lock: environment_lock,
        _verification_executor: Some(verification_executor),
    }
}
