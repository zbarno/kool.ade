use super::super::helpers::EnvSandbox;
use crate::{
    app::welcome::attempt_connect,
    core::git_intent::{GitIntentV1, PublicationModeV1},
    core::planning_work::{self, Work, WorkStatus},
    domain::{ChatMessage, ChatRole},
    persistence::{cancelled_work, task_chats::TaskChats},
};

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed: {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Production boundary: reconnecting a code checkout reloads private planning
/// work, cancellation tombstones, and saved task conversations, then reconciles
/// abandoned in-progress work. A state-store test cannot prove this wiring.
/// Dependencies: temporary local Git and private files only; no services;
/// setup cost is one connected project and a second connection.
#[test]
fn production_reconnect_reloads_saved_reply_cancellation_and_orphan_recovery() {
    let _env = EnvSandbox::enter("startup-orphan-recovery", false);
    let code = super::super::helpers::local_source_repo("startup-orphan-recovery");
    let mut project = attempt_connect(code.to_str().unwrap()).unwrap();
    let store = project.state.planning_store.clone();
    let slug = project.chat_slug.clone();
    let approved_source_sha = git(&code, &["rev-parse", "HEAD"]);

    let mut pending = Work::new(
        "planning:startup-pending-answer".into(),
        "Resume the saved answer".into(),
        "Continue with the answer already provided.".into(),
        "The prior app session ended during planning.".into(),
    );
    pending.status = WorkStatus::InProgress;
    pending.task_intent_uid = Some(pending.uid.clone());
    pending.source_branch = Some("main".into());
    pending.destination_branch = Some("main".into());
    let git_intent = GitIntentV1::approve(
        "root".into(),
        "main".into(),
        approved_source_sha.clone(),
        "main".into(),
        PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    pending.git_intent = Some(git_intent.clone());
    pending.git_intents.insert("root".into(), git_intent);
    let pending_uid = pending.uid.clone();
    let pending_key = pending.key.clone();

    let mut cancelled = Work::new(
        "planning:startup-cancelled".into(),
        "Do not revive cancelled work".into(),
        "This task was cancelled before restart.".into(),
        String::new(),
    );
    let cancelled_uid = cancelled.uid.clone();
    cancelled.status = WorkStatus::InProgress;
    let revision = store.revision().unwrap();
    let revision = planning_work::save_expected(&store, &[pending, cancelled], &revision).unwrap();
    let cancelled_ids =
        std::collections::BTreeSet::from([cancelled_work::planning_id(&cancelled_uid)]);
    cancelled_work::save_expected(&store, &cancelled_ids, &revision).unwrap();
    let active_cancelled_worker =
        crate::persistence::execution_records::try_task_lock(&cancelled_uid)
            .unwrap()
            .expect("the cancelled sibling still has a held worker lease");

    project.task_chats.ensure_loaded(&slug);
    let answer = ChatMessage::new(
        ChatRole::User,
        "Use the already approved release branch.",
        Some(pending_key.clone()),
    );
    let mut retry = ChatMessage::new(
        ChatRole::System,
        "The saved answer is retained for an internal retry.",
        None,
    );
    TaskChats::mark_planning_conflict_retry(&mut retry);
    project
        .task_chats
        .append(&slug, &pending_key, vec![answer, retry])
        .unwrap();
    std::fs::write(
        code.join("baseline-advanced.txt"),
        "new destination baseline\n",
    )
    .unwrap();
    git(&code, &["add", "baseline-advanced.txt"]);
    git(
        &code,
        &[
            "-c",
            "user.name=Recovery Test",
            "-c",
            "user.email=recovery@example.test",
            "commit",
            "-m",
            "advance destination baseline",
        ],
    );
    let advanced_baseline_sha = git(&code, &["rev-parse", "HEAD"]);
    assert_ne!(approved_source_sha, advanced_baseline_sha);
    drop(project);

    let mut restarted = attempt_connect(code.to_str().unwrap()).unwrap();
    assert!(
        restarted
            .cancelled_work
            .contains(&cancelled_work::planning_id(&cancelled_uid))
    );
    let pending = restarted
        .planning_work
        .iter()
        .find(|work| work.uid == pending_uid)
        .expect("production connection reloads the pending work identity");
    assert_eq!(pending.status, WorkStatus::NeedsAttention);
    assert_eq!(pending.task_intent_root_uid(), Some(pending_uid.as_str()));
    let cancelled = restarted
        .planning_work
        .iter()
        .find(|work| work.uid == cancelled_uid)
        .expect("cancelled sibling remains recorded");
    assert_eq!(cancelled.status, WorkStatus::InProgress);
    assert!(
        restarted
            .cancelled_work
            .contains(&cancelled_work::planning_id(&cancelled_uid))
    );
    assert!(
        crate::persistence::execution_records::try_task_lock(&cancelled_uid)
            .unwrap()
            .is_none()
    );
    let pending = restarted
        .planning_work
        .iter()
        .find(|work| work.uid == pending_uid)
        .unwrap();
    assert_eq!(
        pending.git_intent.as_ref().unwrap().source_sha,
        approved_source_sha
    );
    assert_eq!(git(&code, &["rev-parse", "HEAD"]), advanced_baseline_sha);
    restarted.task_chats.ensure_loaded(&restarted.chat_slug);
    assert_eq!(
        restarted
            .task_chats
            .pending_planning_conflict_retries(&pending_key),
        Some(1)
    );
    assert!(restarted.activity.pending.iter().any(|event| {
        event.starts_with("Kool.ad/e reconciled planning work with no active turn")
    }));
    drop(active_cancelled_worker);
    drop(restarted);
    let _ = std::fs::remove_dir_all(code);
}
