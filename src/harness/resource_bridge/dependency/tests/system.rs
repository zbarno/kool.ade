use super::super::policy::decision_allowed;
use crate::harness::{DependencyDecision, DependencyKind, DependencyNeed, PackageEcosystem};

#[test]
fn system_packages_require_one_time_user_authorization() {
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::System,
        package: Some("curl".into()),
        version: Some("7.88.1-10+deb12u14".into()),
        source: Some("debian:bookworm".into()),
        command: "apt-get install -y curl=7.88.1-10+deb12u14".into(),
        reason: "The implementation requires an HTTP client for its task".into(),
        kind: DependencyKind::SystemTool,
        lockfile_identity: None,
        introduced_packages: vec![crate::harness::DependencyPackageIdentity {
            package: "curl".into(),
            version: "7.88.1-10+deb12u14".into(),
            source: "https://deb.debian.org/debian/pool/main/c/curl/curl_7.88.1_amd64.deb".into(),
            integrity: format!("sha256:{}", "a".repeat(64)),
        }],
    };
    for decision in [
        DependencyDecision::AutoAuthorize,
        DependencyDecision::AuthorizeForTask,
        DependencyDecision::AuthorizeForProject,
    ] {
        assert!(!super::super::policy::manager_decision_allowed(
            &need, decision
        ));
    }
    assert!(!decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForProject
    ));
    assert!(decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForTask
    ));
}

#[test]
fn system_tool_request_binds_every_root_package_to_the_bounded_command() {
    let parse = super::super::validation::system_install_packages;
    assert_eq!(
        parse("apt-get install -y nodejs python3", "nodejs", None),
        Some(vec!["nodejs".into(), "python3".into()])
    );
    assert!(parse("apt-get install -y nodejs; id", "nodejs", None).is_none());
    assert!(parse("apt-get install -y python3 nodejs", "nodejs", None).is_none());
    assert!(parse("apt-get install -y nodejs=1.0 python3", "nodejs", None).is_none());
    assert!(parse("apt-get install -y nodejs=1.0", "nodejs", Some("2.0")).is_none());
}
