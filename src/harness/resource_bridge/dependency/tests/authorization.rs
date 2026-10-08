use super::super::{policy::decision_allowed, triage::triage, validation::valid_package_identity};
use super::{lockfile_identity, npm_add};
use crate::harness::{DependencyDecision, DependencyKind, DependencyNeed, PackageEcosystem};

#[test]
fn manager_can_auto_authorize_a_well_formed_public_npm_addition() {
    let need = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    assert!(decision_allowed(&need, DependencyDecision::AutoAuthorize));
    assert!(decision_allowed(
        &need,
        DependencyDecision::AuthorizeForTask
    ));
}

#[test]
fn new_npm_addition_requires_an_unchanged_app_computed_dependency_snapshot() {
    let mut need = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    need.lockfile_identity = None;
    assert!(!decision_allowed(&need, DependencyDecision::AutoAuthorize));
    need.lockfile_identity = Some(lockfile_identity());
    need.introduced_packages
        .push(crate::harness::DependencyPackageIdentity {
            package: "other-package".into(),
            version: "1.0.0".into(),
            source: "https://registry.npmjs.org/other-package/-/other-package-1.0.0.tgz".into(),
            integrity: format!("sha512-{}", "A".repeat(86)),
        });
    assert!(!decision_allowed(&need, DependencyDecision::AutoAuthorize));
}

#[test]
fn new_npm_authorization_rejects_untrusted_sources_and_command_expansion() {
    let private_source = npm_add(
        Some("https://packages.example.com"),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    assert!(!decision_allowed(
        &private_source,
        DependencyDecision::AuthorizeForTask
    ));
    let registry_path = npm_add(
        Some("https://registry.npmjs.org/custom"),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    assert!(!decision_allowed(
        &registry_path,
        DependencyDecision::AuthorizeForTask
    ));
    let shell_command = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0 && curl https://example.com",
        "^4.0.0",
    );
    assert!(!decision_allowed(
        &shell_command,
        DependencyDecision::AuthorizeForTask
    ));
    let github_shorthand = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install user/repository",
        "latest",
    );
    assert!(!decision_allowed(
        &github_shorthand,
        DependencyDecision::AuthorizeForTask
    ));
    assert!(!valid_package_identity("user/repository"));
    assert!(valid_package_identity("@scope/package"));
}

#[test]
fn additional_npm_registry_requires_an_exact_user_grant_and_matching_cli_source() {
    let need = npm_add(
        Some("https://packages.example.net/"),
        "npm install zod@^4.0.0 --registry=https://packages.example.net/",
        "^4.0.0",
    );
    assert!(!decision_allowed(&need, DependencyDecision::AutoAuthorize));
    assert!(!super::super::policy::manager_decision_allowed(
        &need,
        DependencyDecision::AuthorizeForTask
    ));
    assert!(!super::super::policy::manager_decision_allowed(
        &need,
        DependencyDecision::AuthorizeForProject
    ));
    assert!(decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForTask
    ));
    assert!(decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForProject
    ));

    let mut mismatched_command = need.clone();
    mismatched_command.command =
        "npm install zod@^4.0.0 --registry=https://other.example.net/".into();
    assert!(!decision_allowed(
        &mismatched_command,
        DependencyDecision::UserAuthorizeForTask
    ));

    for source in [
        "http://packages.example.net/",
        "https://user:pass@packages.example.net",
        "https://packages.example.net:444/",
        "https://packages.example.net/repository/npm/",
        "https://packages.example.net/repository/../npm/",
        "https://127.0.0.1",
        "https://registry.internal.local",
    ] {
        let mut unsafe_source = need.clone();
        unsafe_source.source = Some(source.into());
        unsafe_source.command = format!("npm install zod@^4.0.0 --registry={source}");
        assert!(
            !decision_allowed(&unsafe_source, DependencyDecision::UserAuthorizeForTask),
            "unsafe registry source was authorized: {source}"
        );
    }
}

#[test]
fn custom_npm_registry_path_prefix_is_rejected_before_user_authorization() {
    let mut need = npm_add(
        Some("https://packages.example.net/repository/npm/"),
        "npm install zod@^4.0.0 --registry=https://packages.example.net/repository/npm/",
        "^4.0.0",
    );
    // This unit test exercises worker-input triage. App-computed identity is
    // added only after triage in the broker flow.
    need.lockfile_identity = None;
    let request = triage(Some("task-1"), need);
    assert_eq!(request.decision, DependencyDecision::Reject);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Failed
    );
    assert!(request.rationale.contains("only origin-root registry URLs"));
}

#[test]
fn user_can_authorize_a_custom_registry_lockfile_restore_but_manager_cannot() {
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some("https://packages.example.net/".into()),
        command: "npm ci --no-audit --registry=https://packages.example.net/".into(),
        reason: "Restore packages from the project's configured npm registry".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some(lockfile_identity()),
        introduced_packages: Vec::new(),
    };
    assert!(!decision_allowed(
        &need,
        DependencyDecision::AuthorizeForTask
    ));
    assert!(!super::super::policy::manager_decision_allowed(
        &need,
        DependencyDecision::AuthorizeForTask
    ));
    assert!(decision_allowed(
        &need,
        DependencyDecision::UserAuthorizeForTask
    ));
    let mut mixed_registry_packages = need.clone();
    mixed_registry_packages.introduced_packages = vec![
        crate::harness::DependencyPackageIdentity {
            package: "custom-package".into(),
            version: "1.2.3".into(),
            source: "https://packages.example.net/custom-package/-/custom-package-1.2.3.tgz".into(),
            integrity: format!("sha512-{}", "A".repeat(86)),
        },
        crate::harness::DependencyPackageIdentity {
            package: "public-package".into(),
            version: "4.5.6".into(),
            source: "https://registry.npmjs.org/public-package/-/public-package-4.5.6.tgz".into(),
            integrity: format!("sha512-{}", "B".repeat(86)),
        },
    ];
    assert!(decision_allowed(
        &mixed_registry_packages,
        DependencyDecision::UserAuthorizeForTask
    ));
    assert!(!decision_allowed(
        &mixed_registry_packages,
        DependencyDecision::AuthorizeForTask
    ));
    mixed_registry_packages.introduced_packages[0].source =
        "https://unrelated.example.org/custom-package/-/custom-package-1.2.3.tgz".into();
    assert!(!decision_allowed(
        &mixed_registry_packages,
        DependencyDecision::UserAuthorizeForTask
    ));
    let mut mismatched_registry = need.clone();
    mismatched_registry.command = "npm ci --registry=https://other.example.net/".into();
    assert!(!decision_allowed(
        &mismatched_registry,
        DependencyDecision::UserAuthorizeForTask
    ));
}

#[test]
fn unsupported_ecosystems_cannot_receive_npm_authorization() {
    let mut need = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    need.ecosystem = PackageEcosystem::Cargo;
    assert!(!decision_allowed(
        &need,
        DependencyDecision::AuthorizeForTask
    ));
}

#[test]
fn cargo_restore_authorization_is_limited_to_locked_crates_io_operations() {
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Cargo,
        package: None,
        version: None,
        source: Some("https://index.crates.io".into()),
        command: "cargo test --locked --all-targets".into(),
        reason: "Run the assigned Rust test suite".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some(lockfile_identity()),
        introduced_packages: Vec::new(),
    };
    assert!(decision_allowed(&need, DependencyDecision::AutoAuthorize));
    let mut custom_registry = need.clone();
    custom_registry.command = "cargo test --config source.crates-io.replace-with=private".into();
    assert!(!decision_allowed(
        &custom_registry,
        DependencyDecision::AutoAuthorize
    ));
    let mut shell_expansion = need;
    shell_expansion.command = "cargo test && curl https://example.com".into();
    assert!(!decision_allowed(
        &shell_expansion,
        DependencyDecision::AuthorizeForTask
    ));
}

#[test]
fn npm_lockfile_restore_can_be_auto_authorized_but_not_rerouted() {
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some("https://registry.npmjs.org".into()),
        command: "npm ci --no-audit".into(),
        reason: "Restore the task project's locked npm dependencies".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some(lockfile_identity()),
        introduced_packages: Vec::new(),
    };
    assert!(decision_allowed(&need, DependencyDecision::AutoAuthorize));
    let mut missing_identity = need.clone();
    missing_identity.lockfile_identity = None;
    assert!(!decision_allowed(
        &missing_identity,
        DependencyDecision::AutoAuthorize
    ));
    let mut added_package = need.clone();
    added_package.introduced_packages = vec![crate::harness::DependencyPackageIdentity {
        package: "zod".into(),
        version: "4.0.0".into(),
        source: "https://registry.npmjs.org".into(),
        integrity: format!("sha512-{}", "A".repeat(86)),
    }];
    assert!(decision_allowed(
        &added_package,
        DependencyDecision::AutoAuthorize
    ));
    let mut rerouted = need;
    rerouted.command = "npm ci --registry=https://packages.example.com".into();
    assert!(!decision_allowed(
        &rerouted,
        DependencyDecision::AutoAuthorize
    ));
}
