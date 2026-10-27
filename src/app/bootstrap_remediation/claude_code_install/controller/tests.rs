use super::progress::advance_progress;
use crate::app::bootstrap_remediation::claude_code_install::ClaudeInstallProgress;

#[test]
fn broker_progress_is_typed_monotonic_and_bounded_to_reviewed_phases() {
    let mut progress = None;
    for phase in [
        ClaudeInstallProgress::Download,
        ClaudeInstallProgress::TrustVerification,
        ClaudeInstallProgress::CurrentUserInstall,
        ClaudeInstallProgress::FreshProviderProbe,
    ] {
        advance_progress(&mut progress, phase);
        assert_eq!(progress, Some(phase));
    }
    advance_progress(&mut progress, ClaudeInstallProgress::Download);
    assert_eq!(progress, Some(ClaudeInstallProgress::FreshProviderProbe));
}
