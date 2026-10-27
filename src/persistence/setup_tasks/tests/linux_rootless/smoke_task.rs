use super::*;

#[test]
fn approved_image_smoke_task_is_unblocked_before_rootless_summary_verifies() {
    let mut store = SetupTaskStoreV1::default();
    let image_task_id = format!("setup:images:general:amd64:{}", "a".repeat(64));
    let initial = snapshot(
        &[
            ("setup:docker:rootless", FindingStatus::NeedsAttention, &[]),
            (
                &image_task_id,
                FindingStatus::NeedsAttention,
                &["setup:docker:engine", "setup:docker:capabilities"],
            ),
        ],
        &["docker"],
        10,
    );
    scan(&mut store, &initial).unwrap();

    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        SetupTaskStateV1::ReadyToFix
    );
    let image = &store.tasks[&image_task_id];
    assert_eq!(image.state, SetupTaskStateV1::ReadyToFix);
    assert!(image.blocked_by.is_empty());
    assert!(
        image
            .dependencies
            .iter()
            .all(|dependency| { dependency.prerequisite_id != "setup:docker:rootless" })
    );
}
