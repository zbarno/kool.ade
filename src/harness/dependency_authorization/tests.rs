use super::{
    DependencyNeed, DependencyResolution, answer, register, remember_once_for_request, take_once,
};
use crate::harness::{
    DependencyDecision, DependencyFailureCategory, DependencyKind, DependencyRequest,
    DependencyRequestStatus, PackageEcosystem,
};

#[test]
fn one_time_grant_is_project_and_task_scoped_and_consumed_by_one_request() {
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: Some("zod".into()),
        version: Some("4.0.0".into()),
        source: Some("https://registry.npmjs.org".into()),
        command: "npm install zod@4.0.0".into(),
        reason: "Validate imported settings data".into(),
        kind: DependencyKind::NewProjectDependency,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    let request = DependencyRequest {
        id: "request-1".into(),
        task_id: "stable-task-uid".into(),
        need: need.clone(),
        category: DependencyFailureCategory::Unknown,
        decision: DependencyDecision::RequiresUserAuthorization,
        rationale: "User decision required".into(),
        risk: "External package source".into(),
        status: DependencyRequestStatus::AwaitingUser,
        preparation: None,
    };
    assert!(remember_once_for_request("project-a", &request));
    assert!(!take_once("project-b", "stable-task-uid", &need));
    assert!(!take_once("project-a", "ticket/path.md", &need));
    let mut different_command = need.clone();
    different_command.command = "npm install zod@4.0.0 --save-prod".into();
    assert!(!take_once(
        "project-a",
        "stable-task-uid",
        &different_command
    ));
    assert!(take_once("project-a", "stable-task-uid", &need));
    assert!(!take_once("project-a", "stable-task-uid", &need));
}

#[test]
fn authorization_ids_allow_one_manager_handoff_and_one_final_answer() {
    let id = format!("dependency-{}", uuid::Uuid::new_v4());
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some("https://registry.npmjs.org".into()),
        command: "npm ci".into(),
        reason: "Restore locked task dependencies".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some("synthetic-lock-identity".into()),
        introduced_packages: Vec::new(),
    };
    let registration = register(&id, &task_id, &need).unwrap();
    let requires_user = DependencyResolution {
        decision: DependencyDecision::RequiresUserAuthorization,
        scope: None,
        rationale: "Manager requests an operator decision".into(),
    };
    let mut changed_need = need.clone();
    changed_need.command = "npm install".into();
    assert!(!answer(&id, &task_id, &changed_need, requires_user.clone()));
    assert!(!answer(&id, "another-task", &need, requires_user.clone()));
    assert!(answer(&id, &task_id, &need, requires_user.clone()));
    assert!(!answer(&id, &task_id, &need, requires_user));
    assert_eq!(
        registration
            .recv_timeout(std::time::Duration::from_millis(20))
            .unwrap()
            .decision,
        DependencyDecision::RequiresUserAuthorization
    );
    let approved = DependencyResolution {
        decision: DependencyDecision::UserAuthorizeForTask,
        scope: Some(crate::harness::DependencyAuthorizationScope::Once),
        rationale: "Approved once".into(),
    };
    assert!(answer(&id, &task_id, &need, approved.clone()));
    assert!(!answer(&id, &task_id, &need, approved));
    assert_eq!(
        registration
            .recv_timeout(std::time::Duration::from_millis(20))
            .unwrap()
            .decision,
        DependencyDecision::UserAuthorizeForTask
    );
    drop(registration);
}

#[test]
fn authorization_registrations_are_bounded_per_task() {
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some("https://registry.npmjs.org".into()),
        command: "npm ci".into(),
        reason: "Restore locked task dependencies".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: Some("synthetic-lock-identity".into()),
        introduced_packages: Vec::new(),
    };
    let registrations = (0..8)
        .map(|_| {
            register(
                &format!("dependency-{}", uuid::Uuid::new_v4()),
                &task_id,
                &need,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        register(
            &format!("dependency-{}", uuid::Uuid::new_v4()),
            &task_id,
            &need
        )
        .is_err()
    );
    drop(registrations);
    assert!(
        register(
            &format!("dependency-{}", uuid::Uuid::new_v4()),
            &task_id,
            &need
        )
        .is_ok()
    );
}
