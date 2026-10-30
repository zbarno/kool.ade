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
