use super::support::{record, work_path};

use crate::core::planning_sync::{
    journal::RecordMutation,
    merge::{MergeResult, apply_mutation},
};

#[test]
fn same_record_nonoverlapping_fields_merge_and_revision_advances() {
    let uid = uuid::Uuid::new_v4();
    let base = record(uid, "old", "original", 1);
    let local = record(uid, "local title", "original", 2);
    let remote = record(uid, "old", "remote detail", 2);
    let mutation = RecordMutation {
        path: work_path(uid),
        record_id: uid.hyphenated().to_string(),
        expected_revision: Some(1),
        base: Some(String::from_utf8(base).unwrap()),
        desired: Some(String::from_utf8(local).unwrap()),
    };
    let MergeResult::Applied(Some(merged)) = apply_mutation(&mutation, Some(&remote)).unwrap()
    else {
        panic!("nonoverlapping changes should merge");
    };
    let merged: serde_json::Value = serde_json::from_slice(&merged).unwrap();
    assert_eq!(merged["data"]["title"], "local title");
    assert_eq!(merged["data"]["detail"], "remote detail");
    assert_eq!(merged["revision"], 3);
}

#[test]
fn same_record_same_field_is_an_explicit_conflict() {
    let uid = uuid::Uuid::new_v4();
    let base = record(uid, "old", "detail", 1);
    let local = record(uid, "local", "detail", 2);
    let remote = record(uid, "remote", "detail", 2);
    let mutation = RecordMutation {
        path: work_path(uid),
        record_id: uid.hyphenated().to_string(),
        expected_revision: Some(1),
        base: Some(String::from_utf8(base).unwrap()),
        desired: Some(String::from_utf8(local).unwrap()),
    };
    assert!(
        matches!(apply_mutation(&mutation, Some(&remote)).unwrap(), MergeResult::Conflict(fields) if fields == ["data.title"])
    );
}

#[test]
fn malformed_record_is_rejected_before_a_merge_fast_path() {
    let uid = uuid::Uuid::new_v4();
    let mutation = RecordMutation {
        path: work_path(uid),
        record_id: uid.hyphenated().to_string(),
        expected_revision: Some(1),
        base: Some("{ malformed".into()),
        desired: Some(String::from_utf8(record(uid, "new", "detail", 2)).unwrap()),
    };

    assert!(apply_mutation(&mutation, Some(b"{ malformed")).is_err());
}

#[test]
fn oversized_remote_record_is_rejected_before_json_parsing() {
    let uid = uuid::Uuid::new_v4();
    let path = work_path(uid);
    let oversized = vec![b' '; 4 * 1024 * 1024 + 1];
    assert!(
        crate::core::planning_sync::merge::validate_record_file(&path, Some(&oversized)).is_err()
    );
}

#[test]
fn noncanonical_uid_path_cannot_bypass_record_validation() {
    let mutation = RecordMutation {
        path: "state/work/not-a-uuid.json".into(),
        record_id: "state/work/not-a-uuid.json".into(),
        expected_revision: None,
        base: None,
        desired: Some("{ malformed".into()),
    };

    assert!(apply_mutation(&mutation, None).is_err());
}

#[test]
fn malformed_planning_item_path_and_content_cannot_bypass_validation() {
    let malformed_path = RecordMutation {
        path: "planning/items/not-a-uuid.md".into(),
        record_id: "planning/items/not-a-uuid.md".into(),
        expected_revision: None,
        base: None,
        desired: Some("malformed content".into()),
    };
    assert!(apply_mutation(&malformed_path, None).is_err());

    let uid = uuid::Uuid::new_v4();
    let malformed_content = RecordMutation {
        path: format!("planning/items/{uid}.md"),
        record_id: uid.hyphenated().to_string(),
        expected_revision: None,
        base: None,
        desired: Some("malformed content".into()),
    };
    assert!(apply_mutation(&malformed_content, None).is_err());
}

#[test]
fn valid_planning_item_content_survives_the_record_validation_path() {
    let uid = uuid::Uuid::new_v4();
    let content = format!(
        "---\nkoolade-item-content: {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"id\":\"CLR-001\",\"decisionBrief\":null}}\n---\n\n# CLR-001\n\n## Question\n\n<!-- koolade-item-question -->\nquestion\n<!-- /koolade-item-question -->\n\n## Reason\n\n<!-- koolade-item-reason -->\nreason\n<!-- /koolade-item-reason -->\n\n## Recommendation\n\n<!-- koolade-item-recommendation -->\nrecommendation\n<!-- /koolade-item-recommendation -->\n\n## Evidence\n\n<!-- koolade-item-evidence -->\nevidence\n<!-- /koolade-item-evidence -->\n"
    );
    let mutation = RecordMutation {
        path: format!("planning/items/{uid}.md"),
        record_id: uid.hyphenated().to_string(),
        expected_revision: None,
        base: None,
        desired: Some(content.clone()),
    };

    assert_eq!(
        apply_mutation(&mutation, None).unwrap(),
        MergeResult::Applied(Some(content.into_bytes()))
    );
}
