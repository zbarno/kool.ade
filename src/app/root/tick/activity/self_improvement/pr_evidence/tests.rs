use super::*;

fn parent(status: Option<Status>, task_uid: &str) -> SelfImprovementParentTaskProgressV1 {
    SelfImprovementParentTaskProgressV1 {
        task_uid: task_uid.into(),
        phase: crate::core::lifecycle::TaskPhase::Ready,
        implementation_sha256: None,
        code_review_uid: None,
        runtime_qa_uid: None,
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_status: status,
        pr_url: Some(format!(
            "https://github.com/zbarno/kool.ade/pull/{task_uid}"
        )),
        pr_head_sha: None,
        pr_base_branch: Some("main".into()),
        pr_merged_commit_sha: None,
        pr_merged_at: None,
    }
}

fn merged_parent(
    task_uid: &str,
    sha: char,
    merged_at: &str,
    base: &str,
) -> SelfImprovementParentTaskProgressV1 {
    let mut parent = parent(Some(Status::Merged), task_uid);
    parent.pr_base_branch = Some(base.into());
    parent.pr_merged_commit_sha = Some(sha.to_string().repeat(40));
    parent.pr_merged_at = Some(merged_at.into());
    parent
}

#[test]
fn aggregate_merge_sha_uses_latest_merge_after_every_parent_targets_destination() {
    let progress = [
        merged_parent("1", 'a', "2026-10-18T10:00:00Z", "main"),
        merged_parent("2", 'b', "2026-10-18T10:01:00Z", "main"),
    ];
    assert_eq!(
        aggregate_merged_parent_sha(&progress, Some("refs/heads/main")),
        Some("b".repeat(40))
    );
}

#[test]
fn aggregate_merge_sha_fails_closed_when_any_parent_evidence_is_incomplete_or_misdirected() {
    let mut progress = [
        merged_parent("1", 'a', "2026-10-18T10:00:00Z", "main"),
        merged_parent("2", 'b', "2026-10-18T10:01:00Z", "main"),
    ];
    progress[1].pr_merged_at = None;
    assert_eq!(aggregate_merged_parent_sha(&progress, Some("main")), None);
    progress[1] = merged_parent("2", 'b', "2026-10-18T10:01:00Z", "develop");
    assert_eq!(aggregate_merged_parent_sha(&progress, Some("main")), None);
}

#[test]
fn closed_unmerged_aggregate_can_resume_when_a_parent_pr_is_reopened() {
    let mut progress = [
        parent(Some(Status::ClosedUnmerged), "1"),
        parent(Some(Status::Merged), "2"),
    ];
    assert_eq!(reopened_parent(&progress), None);
    progress[0].pr_status = Some(Status::Opened);
    assert_eq!(reopened_parent(&progress), Some("1".into()));
    progress[1].pr_status = None;
    assert_eq!(reopened_parent(&progress), None);
}

#[test]
fn ancestry_verification_accepts_only_provider_confirmed_ancestry() {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    let progress = [
        merged_parent("1", 'a', "2026-10-18T10:00:00Z", "main"),
        merged_parent("2", 'b', "2026-10-18T10:01:00Z", "main"),
    ];
    for (result, expected) in [("ahead", true), ("identical", true), ("diverged", false)] {
        let temp = tempfile::tempdir().unwrap();
        let gh = temp.path().join("gh");
        let args_path = temp.path().join("args");
        std::fs::write(
            &gh,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{result}'\n",
                args_path.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&gh).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&gh, permissions).unwrap();
        assert_eq!(
            verify_parent_merge_ancestry(
                "https://github.com/zbarno/kool.ade",
                &progress,
                &"b".repeat(40),
                Path::new(&gh),
            ),
            expected,
            "provider comparison result {result}"
        );
        assert_eq!(
            std::fs::read_to_string(args_path).unwrap(),
            format!(
                "api\nrepos/zbarno/kool.ade/compare/{}...{}\n--jq\n.status\n--hostname\ngithub.com\n",
                "a".repeat(40),
                "b".repeat(40)
            )
        );
    }
}

#[test]
fn ancestry_provider_check_is_backgrounded_and_deduplicated() {
    use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let gh = temp.path().join("gh");
    let calls = temp.path().join("calls");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nsleep 0.2\nprintf 'ahead\\n'\n",
            calls.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&gh, permissions).unwrap();

    let progress = [
        merged_parent("1", 'a', "2026-10-18T10:00:00Z", "main"),
        merged_parent("2", 'b', "2026-10-18T10:01:00Z", "main"),
    ];
    let latest = "b".repeat(40);
    let mut activity = crate::app::manager::WorkspaceActivity::default();
    assert!(
        !parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        )
        .unwrap()
    );
    assert!(
        !parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        )
        .unwrap()
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut verified = false;
    while std::time::Instant::now() < deadline {
        verified = parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        )
        .unwrap();
        if verified {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(verified, "background provider comparison should complete");
    assert_eq!(std::fs::read_to_string(calls).unwrap(), "x");
}

#[test]
fn failed_ancestry_check_uses_keyed_retry_backoff() {
    use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let gh = temp.path().join("gh");
    let calls = temp.path().join("calls");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nprintf 'diverged\\n'\n",
            calls.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&gh, permissions).unwrap();

    let progress = [
        merged_parent("1", 'a', "2026-10-18T10:00:00Z", "main"),
        merged_parent("2", 'b', "2026-10-18T10:01:00Z", "main"),
    ];
    let latest = "b".repeat(40);
    let mut activity = crate::app::manager::WorkspaceActivity::default();
    assert!(
        !parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        )
        .unwrap()
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        ) {
            Err(_) => break,
            Ok(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => panic!("expected provider failure before deadline, got {result:?}"),
        }
    }
    assert!(
        !parent_merge_ancestry_verified(
            &mut activity,
            "https://github.com/zbarno/kool.ade",
            &progress,
            &latest,
            Path::new(&gh),
        )
        .unwrap()
    );
    assert_eq!(std::fs::read_to_string(calls).unwrap(), "x");
}

#[test]
fn ancestry_provider_process_is_killed_at_its_timeout() {
    use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let gh = temp.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexec sleep 5\n").unwrap();
    let mut permissions = std::fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&gh, permissions).unwrap();

    let started = std::time::Instant::now();
    let error = ancestry_compare_with_timeout(
        Path::new(&gh),
        "repos/zbarno/kool.ade/compare/a...b",
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(error.contains("timeout"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timed-out provider subprocess should be killed and reaped"
    );
}

#[test]
fn one_closed_parent_does_not_close_aggregate_while_sibling_pr_is_open() {
    let progress = [parent(Some(Status::ClosedUnmerged), "1"), parent(None, "2")];
    assert_eq!(closed_unmerged_parent(&progress), None);
}

#[test]
fn aggregate_closes_after_every_parent_pr_is_terminal() {
    let progress = [
        parent(Some(Status::ClosedUnmerged), "1"),
        parent(Some(Status::Merged), "2"),
    ];
    assert_eq!(
        closed_unmerged_parent(&progress),
        Some((
            "1".into(),
            "https://github.com/zbarno/kool.ade/pull/1".into()
        ))
    );
}
