use super::*;

struct RestoreSshCommand(Option<std::ffi::OsString>);

impl RestoreSshCommand {
    fn capture() -> Self {
        Self(std::env::var_os("GIT_SSH_COMMAND"))
    }
}

impl Drop for RestoreSshCommand {
    fn drop(&mut self) {
        // SAFETY: repository tests run serially, so restoring the prior value cannot race a test.
        unsafe {
            if let Some(command) = self.0.as_ref() {
                std::env::set_var("GIT_SSH_COMMAND", command);
            } else {
                std::env::remove_var("GIT_SSH_COMMAND");
            }
        }
    }
}

pub(super) struct Sandbox {
    pub(super) root: PathBuf,
    pub(super) repo: PathBuf,
    pub(super) gh: PathBuf,
    pub(super) ticket: String,
    _ssh_command_restore: RestoreSshCommand,
}
impl Sandbox {
    pub(super) fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    pub(super) fn advance_remote(&self) -> String {
        let peer = self.root.join("peer");
        self.git(
            &self.root,
            &[
                "clone",
                "-q",
                "-b",
                "main",
                self.root.join("remote.git").to_str().unwrap(),
                peer.to_str().unwrap(),
            ],
        );
        self.git(&peer, &["config", "user.name", "Fixture"]);
        self.git(&peer, &["config", "user.email", "fixture@example.test"]);
        fs::write(peer.join("upstream.txt"), "latest upstream\n").unwrap();
        self.git(&peer, &["add", "."]);
        self.git(&peer, &["commit", "-qm", "upstream update"]);
        self.git(&peer, &["push", "-q", "origin", "main"]);
        self.git(&peer, &["rev-parse", "HEAD"])
    }
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-implementation-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&o.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.test"]);
        let ticket =
            ".koolade-packet/planning/tasks/feature/001-implement-ticket-behavior.md".to_owned();
        fs::create_dir_all(repo.join(".koolade-packet/planning/tasks/feature")).unwrap();
        fs::write(repo.join(&ticket), "# Implement ticket behavior\n\n## Acceptance criteria\n\n- File contains implemented.\n").unwrap();
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
        // SAFETY: repository tests run serially, and the RAII guard restores this value on drop.
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
        fs::write(&gh, "#!/bin/sh\nroot=$(dirname \"$0\")\nif [ -f \"$root/offline\" ]; then echo 'simulated GitHub unavailable' >&2; exit 1; fi\nif [ \"$1\" = run ] && [ \"$2\" = list ]; then\n commit=''\n while [ \"$#\" -gt 0 ]; do if [ \"$1\" = --commit ]; then shift; commit=$1; fi; shift; done\n printf '[{\"headSha\":\"%s\",\"status\":\"completed\",\"conclusion\":\"success\",\"workflowName\":\"fixture\",\"createdAt\":\"2099-01-01T00:00:00Z\",\"url\":\"https://github.com/fixture/repo/actions/runs/1\"}]\\n' \"$commit\"\nelif [ \"$2\" = list ]; then\n if [ -f \"$root/pr-created\" ]; then echo '[{\"url\":\"https://github.com/fixture/repo/pull/1\",\"state\":\"OPEN\"}]'; else echo '[]'; fi\nelse\n printf '%s\\n' \"$@\" > \"$root/pr-args\"\n echo created >> \"$root/pr-created\"\n echo 'https://github.com/fixture/repo/pull/1'\nfi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&gh, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root,
            repo,
            gh,
            ticket,
            _ssh_command_restore: ssh_command_restore,
        }
    }
    pub(super) fn run(
        &self,
        mode: &'static str,
        calls: Arc<AtomicUsize>,
    ) -> anyhow::Result<Implementation> {
        let (tx, _rx) = mpsc::channel();
        run_with_gh(
            &self.repo,
            &self.ticket,
            &Fixture { mode, calls },
            Arc::new(AtomicBool::new(false)),
            tx,
            self.gh.to_str().unwrap(),
        )
    }
    pub(super) fn run_with_publication_policy(
        &self,
        fixture_mode: &'static str,
        calls: Arc<AtomicUsize>,
        publication_mode: PublicationMode,
        auto_publish_gate: Option<Arc<AtomicBool>>,
        require_independent_checks: bool,
    ) -> anyhow::Result<Implementation> {
        let (tx, _rx) = mpsc::channel();
        run_with_project_options(
            &self.repo,
            &self.repo,
            &self.ticket,
            RunOptions {
                harness: &Fixture {
                    mode: fixture_mode,
                    calls,
                },
                cancel: Arc::new(AtomicBool::new(false)),
                progress: tx,
                gh: self.gh.to_str().unwrap(),
                publication_mode,
                require_independent_checks,
                user_context: None,
                auto_publish_gate,
                claim_lease: None,
            },
        )
    }
    pub(super) fn make_evidence_only(&self) {
        fs::write(
            self.repo.join(&self.ticket),
            "# Conduct audit\n\nThis ticket lands zero planner code.\n\nAll source files are explicitly unchanged.\n\nNo product-repo file may be created or modified by this ticket.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n",
        )
        .unwrap();
        self.git(&self.repo, &["add", &self.ticket]);
        self.git(&self.repo, &["commit", "-qm", "define evidence-only audit"]);
        self.git(&self.repo, &["push", "-q", "origin", "main"]);
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        if let Ok(repo) = self.repo.canonicalize() {
            let project_id = crate::persistence::project_slug(&repo);
            let _ = fs::remove_dir_all(crate::persistence::project_dir(&project_id));
        }
    }
}

pub(super) fn completed_cleanup_fixture(s: &Sandbox) -> Implementation {
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    let head = state.verified_head.as_deref().unwrap();
    s.git(
        &state.task_repository,
        &[
            "push",
            s.root.join("remote.git").to_str().unwrap(),
            &format!("{head}:refs/heads/main"),
        ],
    );
    state.status = ImplementationStatus::Completed;
    state.pr_state = Some(PullRequestState::Merged);
    state.merged_commit = state.verified_head.clone();
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
    state
}
