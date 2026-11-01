use super::*;

#[test]
fn feature_uid_overrides_a_reused_display_id() {
    let mut work = Work::new(
        "task:old-feature".into(),
        "Plan the original feature".into(),
        "Original request".into(),
        String::new(),
    );
    work.feature_id = Some("F1".into());
    work.feature_uid = Some("00000000-0000-4000-8000-000000000001".into());

    assert!(!belongs_to_feature(
        &work,
        "F1",
        Some("00000000-0000-4000-8000-000000000002")
    ));
    assert!(belongs_to_feature(
        &work,
        "F9",
        Some("00000000-0000-4000-8000-000000000001")
    ));

    work.feature_uid = None;
    assert!(!belongs_to_feature(&work, "F1", Some("new-current-uid")));
    assert!(belongs_to_feature(&work, "F1", None));
}
