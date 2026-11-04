use super::*;

#[path = "sandbox/cleanup.rs"]
mod cleanup;
#[path = "sandbox/create.rs"]
mod create;
#[path = "sandbox/lifecycle.rs"]
mod lifecycle;
#[path = "sandbox/published_specs.rs"]
mod published_specs;
#[path = "sandbox/verification.rs"]
mod verification;
pub(super) use cleanup::completed_cleanup_fixture;
pub(super) use verification::install_verification_executor;
pub(super) fn seed_lifecycle_task(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    ticket: &str,
    ticket_text: &str,
) {
    lifecycle::seed_task(planning_store, ticket, ticket_text);
}
pub(super) fn write_lifecycle_task_with_main_intent(
    sandbox: &Sandbox,
    ticket: &str,
    ticket_text: &str,
    display_id: &str,
    title: &str,
) {
    let source_sha = sandbox.git(&sandbox.repo, &["rev-parse", "refs/heads/main"]);
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown_with_parent(
        ticket_text,
        None,
        display_id,
        title,
        Some(&batch_uid),
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let targets = crate::core::workflow::BranchTargets {
        source: "main".into(),
        destination: "main".into(),
    };
    let intent = crate::core::git_intent::GitIntentV1::approve(
        "root".into(),
        "main".into(),
        source_sha.clone(),
        "main".into(),
        crate::core::git_intent::PublicationModeV1::CreatePullRequest,
        None,
    )
    .unwrap();
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new())
        .unwrap()
        .with_branch_targets(Some(&targets))
        .unwrap()
        .with_source_sha(Some(&source_sha))
        .unwrap()
        .with_git_intent(Some(&intent))
        .unwrap();
    let markdown = crate::artifacts::task_docs::embed(&markdown, &metadata).unwrap();
    sandbox
        .planning_store
        .atomic_write(ticket, markdown.as_bytes())
        .unwrap();
    seed_lifecycle_task(&sandbox.planning_store, ticket, ticket_text);
}
pub(super) fn approve_ticket_source(sandbox: &Sandbox, source: &str, destination: &str) {
    let source_ref = format!("refs/heads/{source}");
    let source_sha = sandbox.git(&sandbox.repo, &["rev-parse", &source_ref]);
    approve_ticket_at_source_sha(sandbox, source, destination, &source_sha);
}
pub(super) fn approve_ticket_at_source_sha(
    sandbox: &Sandbox,
    source: &str,
    destination: &str,
    source_sha: &str,
) {
    approve_ticket_git_intent(sandbox, &sandbox.ticket, source, destination, source_sha);
}
pub(super) fn approve_ticket_git_intent(
    sandbox: &Sandbox,
    ticket: &str,
    source: &str,
    destination: &str,
    source_sha: &str,
) {
    let raw = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
    let mut metadata = crate::artifacts::task_docs::parse_metadata(&raw)
        .unwrap()
        .expect("fixture task Git intent");
    let intent = crate::core::git_intent::GitIntentV1::approve(
        metadata.repository_id.clone(),
        source.into(),
        source_sha.into(),
        destination.into(),
        crate::core::git_intent::PublicationModeV1::CreatePullRequest,
        None,
    )
    .unwrap();
    metadata.source_branch = Some(source.into());
    metadata.destination_branch = Some(destination.into());
    metadata.source_sha = Some(source_sha.into());
    metadata.git_intent = Some(intent);
    let updated = crate::artifacts::task_docs::embed(&raw, &metadata).unwrap();
    sandbox
        .planning_store
        .atomic_write(ticket, updated.as_bytes())
        .unwrap();
}
pub(super) fn approve_current_task_source(sandbox: &Sandbox, ticket: &str) {
    // Reconciliation fixtures explicitly approve the checkout's current
    // source only before the first execution attempt. A resumed task must keep
    // the exact previously approved intent.
    let state_dir = crate::core::implementation::state_dir(&sandbox.planning_store.root, ticket)
        .expect("fixture task state path");
    if state_dir.join("state.json").exists() {
        return;
    }
    let raw = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
    let Some(metadata) = crate::artifacts::task_docs::parse_metadata(&raw)
        .unwrap()
        .filter(|metadata| metadata.git_intent.is_some())
    else {
        return;
    };
    let (Some(source), Some(destination)) = (
        metadata.source_branch.as_deref(),
        metadata.destination_branch.as_deref(),
    ) else {
        return;
    };
    let source_ref = format!("refs/heads/{source}^{{commit}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &source_ref])
        .current_dir(&sandbox.repo)
        .output()
        .unwrap();
    if output.status.success() {
        let source_sha = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        approve_ticket_git_intent(sandbox, ticket, source, destination, &source_sha);
    }
}
pub(super) fn refresh_task_spec(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    ticket: &str,
) {
    lifecycle::refresh_unstarted_task_spec(planning_store, ticket);
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
    pub(super) planning_store: crate::artifacts::planning_store::PlanningStore,
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
        // Sandbox::git supplies the fixture identity with -c, so these
        // ephemeral peers need no persistent identity config writes.
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
    pub(super) fn ticket_path(&self) -> PathBuf {
        self.planning_store.root.join(&self.ticket)
    }
    pub(super) fn run(
        &self,
        mode: &'static str,
        calls: Arc<AtomicUsize>,
    ) -> anyhow::Result<Implementation> {
        refresh_task_spec(&self.planning_store, &self.ticket);
        let (tx, _rx) = mpsc::channel();
        super::run_with_planning_store_options(
            &self.planning_store,
            &self.planning_store.root,
            &self.repo,
            &self.ticket,
            RunOptions {
                harness: &Fixture { mode, calls },
                cancel: Arc::new(AtomicBool::new(false)),
                progress: tx,
                gh: self.gh.to_str().unwrap(),
                publication_mode: PublicationMode::CreatePullRequest,
                require_independent_checks: false,
                user_context: None,
                auto_publish_gate: None,
                claim_lease: None,
            },
        )
    }
    pub(super) fn run_options(
        &self,
        harness: &dyn crate::harness::AiHarness,
        cancel: Arc<AtomicBool>,
        progress: Sender<LiveProgress>,
        auto_publish: bool,
        require_independent_checks: bool,
    ) -> anyhow::Result<Implementation> {
        refresh_task_spec(&self.planning_store, &self.ticket);
        super::run_with_planning_store_options(
            &self.planning_store,
            &self.planning_store.root,
            &self.repo,
            &self.ticket,
            RunOptions {
                harness,
                cancel,
                progress,
                gh: self.gh.to_str().unwrap(),
                publication_mode: if auto_publish {
                    PublicationMode::AutoPublish
                } else {
                    PublicationMode::CreatePullRequest
                },
                require_independent_checks,
                user_context: None,
                auto_publish_gate: None,
                claim_lease: None,
            },
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
        refresh_task_spec(&self.planning_store, &self.ticket);
        let (tx, _rx) = mpsc::channel();
        super::run_with_planning_store_options(
            &self.planning_store,
            &self.planning_store.root,
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
        let path = self.ticket_path();
        let previous = fs::read_to_string(&path).unwrap();
        let ticket_text = "# Conduct audit\n\nFeature: F1\n\nThis ticket lands zero planner code.\n\nAll source files are explicitly unchanged.\n\nNo product-repo file may be created or modified by this ticket.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n";
        let updated = crate::domain::ArtifactIdentity::preserve_markdown(
            ticket_text,
            Some(&previous),
            "TASK-001",
            "Conduct audit",
        )
        .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&updated)
            .unwrap()
            .expect("evidence-only task identity");
        let metadata = crate::artifacts::task_docs::parse_metadata(&previous)
            .unwrap()
            .expect("seeded task metadata");
        metadata.validate(Some(&identity)).unwrap();
        let updated = crate::artifacts::task_docs::embed(&updated, &metadata).unwrap();
        self.planning_store
            .atomic_write(&self.ticket, updated.as_bytes())
            .unwrap();
        refresh_task_spec(&self.planning_store, &self.ticket);
    }
}
