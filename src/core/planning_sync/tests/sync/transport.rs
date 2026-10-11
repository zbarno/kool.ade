use super::super::support::{Fixture, work_path};
use crate::core::planning_sync::{
    PlanningSyncState,
    runner::{prepare_local_intent, sync_with_state_root},
};
use std::fs;

#[cfg(unix)]
#[test]
fn rejected_remote_push_retains_the_journaled_operation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let hook = fixture.remote.join("hooks/pre-receive");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'remote write permission denied' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "rejected change");
    let state_root = fixture.state_root("read-only");
    crate::core::planning_sync::runner::prepare_local_intent(
        &fixture.first,
        &state_root,
        "rejected operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();
    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();
    assert_eq!(status.state, PlanningSyncState::ReadOnly);
    assert_eq!(status.pending_operations, 1);
}

#[cfg(unix)]
#[test]
fn ssh_public_key_failure_is_reported_as_auth_required() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let hook = fixture.remote.join("hooks/pre-receive");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'Permission denied (publickey).' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "auth rejected");
    let state_root = fixture.state_root("auth-required");
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "auth operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::AuthRequired);
    assert_eq!(status.pending_operations, 1);
}

#[cfg(unix)]
#[test]
fn local_pre_push_hook_can_block_a_shared_sync() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let hook = fixture.first.join(".git/hooks/pre-push");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'local safety hook stopped push' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.write_work(
        &fixture.first,
        fixture.work_id,
        "title",
        "hook must stop this",
    );
    let state_root = fixture.state_root("pre-push-hook");
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "hook-protected operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(status.state, PlanningSyncState::Pending);
    assert_eq!(status.pending_operations, 1);
    assert_eq!(
        fixture.remote_record(fixture.work_id)["data"]["title"],
        "original"
    );
}

#[cfg(unix)]
#[test]
fn stale_push_reapplies_after_remote_advances_between_fetch_and_push() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "local title");
    let state_root = fixture.state_root("real-race");
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "local intent",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    fixture.write_work(
        &fixture.second,
        fixture.work_id,
        "detail",
        "concurrent remote detail",
    );
    super::super::support::commit_all(&fixture.second, "concurrent remote change");
    let hook = fixture.first.join(".git/hooks/pre-push");
    let second_repo = fixture.second.display();
    fs::write(
        &hook,
        format!("#!/bin/sh\nset -eu\ngit -C '{second_repo}' push origin HEAD:refs/heads/main\n"),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(
        status.state,
        PlanningSyncState::Synced,
        "{:?}",
        status.detail
    );
    assert_eq!(status.pending_operations, 0);
    let remote = fixture.remote_record(fixture.work_id);
    assert_eq!(remote["data"]["title"], "local title");
    assert_eq!(remote["data"]["detail"], "concurrent remote detail");
}

#[cfg(unix)]
#[test]
fn concurrent_remote_advance_retries_three_times_then_keeps_pending_operation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let count = fixture.root.join("pre-receive-count");
    let hook = fixture.remote.join("hooks/pre-receive");
    let count_path = count.display();
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nset -eu\ncount=0\n[ ! -f '{count_path}' ] || count=$(cat '{count_path}')\ncount=$((count + 1))\nprintf '%s' \"$count\" > '{count_path}'\necho 'non-fast-forward: simulated concurrent update' >&2\nexit 1\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.write_work(&fixture.first, fixture.work_id, "title", "retry me");
    let state_root = fixture.state_root("retry-exhaustion");
    prepare_local_intent(
        &fixture.first,
        &state_root,
        "retry operation",
        &[work_path(fixture.work_id)],
    )
    .unwrap()
    .unwrap();

    let status = sync_with_state_root(&fixture.first, &state_root).unwrap();

    assert_eq!(
        status.state,
        PlanningSyncState::Pending,
        "unexpected sync status: {:?}",
        status.detail
    );
    assert_eq!(status.pending_operations, 1);
    assert_eq!(fs::read_to_string(count).unwrap(), "3");
}
