use super::*;

#[test]
fn navigation_resumes_and_unknown_session_is_preserved() {
    // Real atomic persistence boundary, temporary directory only. No external
    // dependencies. Enum tests cannot establish restart behavior or preservation.
    let root = std::env::temp_dir().join(format!("koolade-onboarding-{}", uuid::Uuid::new_v4()));
    let path = root.join("onboarding.json");
    let mut session = Onboarding::load(path.clone());
    assert_eq!(session.step, Step::Start);
    session.go(Step::Manager);
    session.go(Step::Readiness);
    let restored = Onboarding::load(path.clone());
    assert_eq!(restored.step, Step::Readiness);
    assert_eq!(restored.revision, 2);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("permission"));
    std::fs::write(&path, b"unknown future data").unwrap();
    let mut unknown = Onboarding::load(path.clone());
    assert!(unknown.error.is_some());
    unknown.go(Step::Advanced);
    assert_eq!(std::fs::read(path).unwrap(), b"unknown future data");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_preserves_navigation_and_handoff_references_without_authority() {
    // Real private session serialization across restart; temporary files only.
    // An in-memory enum assertion cannot prove durable recovery references.
    let root = std::env::temp_dir().join(format!("koolade-session-{}", uuid::Uuid::new_v4()));
    let path = root.join("onboarding.json");
    let mut session = Onboarding::load(path.clone());
    let id = uuid::Uuid::new_v4();
    session.opening_workspace_id = Some(id);
    session.pending_interview = Some(InterviewRef {
        workspace_id: id,
        work_key: "saved-idea".into(),
    });
    session.active_setup_task_id = Some("setup:docker:rootless".into());
    session.pending_operation_task_id = session.active_setup_task_id.clone();
    session.go(Step::NewProject);
    let restored = Onboarding::load(path.clone());
    assert_eq!(restored.mode, Mode::Resume);
    assert_eq!(restored.selected_entry_path, Some(EntryPath::NewProject));
    assert_eq!(restored.opening_workspace_id, Some(id));
    let interview = restored.pending_interview.unwrap();
    assert_eq!(interview.workspace_id, id);
    assert_eq!(interview.work_key, "saved-idea");
    assert_eq!(
        restored.pending_operation_task_id,
        restored.active_setup_task_id
    );
    let mut future: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    future["version"] = 2.into();
    let bytes = serde_json::to_vec(&future).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut unsupported = Onboarding::load(path.clone());
    unsupported.go(Step::Advanced);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_dir_all(root).unwrap();
}
