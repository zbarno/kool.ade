use super::*;

#[path = "sandbox/cleanup.rs"]
mod cleanup;
#[path = "sandbox/create.rs"]
mod create;
#[path = "sandbox/lifecycle.rs"]
mod lifecycle;
#[path = "sandbox/verification.rs"]
mod verification;
pub(super) use cleanup::completed_cleanup_fixture;
pub(super) use verification::install_verification_executor;
pub(super) fn seed_lifecycle_task(repo: &Path, ticket: &str, ticket_text: &str) {
    lifecycle::seed_task(repo, ticket, ticket_text);
}
pub(super) fn refresh_task_spec(repo: &Path, ticket: &str) {
    lifecycle::refresh_unstarted_task_spec(repo, ticket);
}
struct RestoreSshCommand(Option<std::ffi::OsString>);

impl RestoreSshCommand {
    fn capture() -> Self {
        Self(std::env::var_os("GIT_SSH_COMMAND"))
    }
}

struct RestoreKooladeHome(Option<std::ffi::OsString>);

impl RestoreKooladeHome {
    fn capture() -> Self {
        Self(std::env::var_os("KOOLADE_HOME"))
    }
}

impl Drop for RestoreKooladeHome {
    fn drop(&mut self) {
        // SAFETY: this fixture holds the shared cross-process environment lock.
        unsafe {
            if let Some(home) = self.0.as_ref() {
                std::env::set_var("KOOLADE_HOME", home);
            } else {
                std::env::remove_var("KOOLADE_HOME");
            }
        }
    }
}

impl Drop for RestoreSshCommand {
    fn drop(&mut self) {
        // SAFETY: this fixture holds the shared cross-process environment lock.
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
    latest_peer: std::sync::Mutex<Option<PathBuf>>,
    _koolade_home_restore: RestoreKooladeHome,
    _ssh_command_restore: RestoreSshCommand,
    _environment_lock: crate::test_support::ProcessEnvironmentLock,
    _verification_executor: Option<super::super::runner::TestVerificationExecutorGuard>,
}
impl Sandbox {
    pub(super) fn use_production_verification(&mut self) {
        self._verification_executor.take();
    }

    pub(super) fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
            ])
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
        // Each remote advance uses its own clone. Some recovery scenarios
        // advance the same fixture more than once, and parallel test runs
        // must never contend over a fixed peer checkout path.
        let peer = self.root.join(format!("peer-{}", uuid::Uuid::new_v4()));
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
        let upstream = peer.join("upstream.txt");
        let contents = fs::read_to_string(&upstream).unwrap_or_default();
        let next_contents = if contents.is_empty() {
            "latest upstream\n".to_owned()
        } else {
            format!("{contents}remote advance\n")
        };
        fs::write(upstream, next_contents).unwrap();
        self.git(&peer, &["add", "."]);
        self.git(&peer, &["commit", "-qm", "upstream update"]);
        self.git(&peer, &["push", "-q", "origin", "main"]);
        let head = self.git(&peer, &["rev-parse", "HEAD"]);
        *self.latest_peer.lock().unwrap() = Some(peer);
        head
    }
    pub(super) fn latest_peer(&self) -> PathBuf {
        self.latest_peer
            .lock()
            .unwrap()
            .clone()
            .expect("advance_remote must create a peer before it is used")
    }
    pub(super) fn new() -> Self {
        create::create(&[])
    }
    pub(super) fn new_with_initial_files(initial_files: &[(&str, &str)]) -> Self {
        create::create(initial_files)
    }
    pub(super) fn run(
        &self,
        mode: &'static str,
        calls: Arc<AtomicUsize>,
    ) -> anyhow::Result<Implementation> {
        refresh_task_spec(&self.repo, &self.ticket);
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
        refresh_task_spec(&self.repo, &self.ticket);
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
        let path = self.repo.join(&self.ticket);
        let previous = fs::read_to_string(&path).unwrap();
        let ticket_text = "# Conduct audit\n\nFeature: F1\n\nThis ticket lands zero planner code.\n\nAll source files are explicitly unchanged.\n\nNo product-repo file may be created or modified by this ticket.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n";
        let updated = crate::domain::ArtifactIdentity::preserve_markdown(
            ticket_text,
            Some(&previous),
            "TASK-001",
            "Conduct audit",
        )
        .unwrap();
        fs::write(path, &updated).unwrap();
        self.git(&self.repo, &["add", &self.ticket]);
        self.git(&self.repo, &["commit", "-qm", "define evidence-only audit"]);
        self.git(&self.repo, &["push", "-q", "origin", "main"]);
    }
}
