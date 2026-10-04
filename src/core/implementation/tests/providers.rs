use super::*;

pub(super) struct ImmediateCheck(pub(super) bool);
impl checks::Provider for ImmediateCheck {
    fn name(&self) -> &'static str {
        "Fixture checks"
    }

    fn repository(&self, _remote: &str) -> Option<String> {
        Some("github.com/fixture/repo".into())
    }

    fn check(
        &self,
        runner: &Runner,
        cwd: &Path,
        repository: &str,
        commit: &str,
    ) -> anyhow::Result<checks::ResultState> {
        assert_eq!(repository, "github.com/fixture/repo");
        let reference = checks::candidate_ref("fixture", commit);
        let remote = runner.git(cwd, &["ls-remote", "origin", &reference])?;
        assert!(
            remote.contains(commit),
            "candidate was not pushed before checking"
        );
        Ok(if self.0 {
            checks::ResultState::Passed
        } else {
            checks::ResultState::Failed("fixture workflow failed".into())
        })
    }
}

pub(super) struct DisablingCheck(pub(super) Arc<AtomicBool>);
impl checks::Provider for DisablingCheck {
    fn name(&self) -> &'static str {
        "Disable during checks"
    }

    fn repository(&self, _remote: &str) -> Option<String> {
        Some("github.com/fixture/repo".into())
    }

    fn check(
        &self,
        _runner: &Runner,
        _cwd: &Path,
        _repository: &str,
        _commit: &str,
    ) -> anyhow::Result<checks::ResultState> {
        self.0.store(false, Ordering::SeqCst);
        Ok(checks::ResultState::Passed)
    }
}
