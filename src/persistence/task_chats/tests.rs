use super::{TaskChats, read};
use crate::domain::{ChatMessage, ChatRole};

#[test]
fn drain_recovers_an_unsaved_reply_once_the_peer_releases_the_lock() {
    let dir = std::env::temp_dir().join(format!("packet_task_drain_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("task-conversations.lock");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (acquire_tx, acquire_rx) = std::sync::mpsc::channel::<()>();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let hold = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        ready_tx.send(()).unwrap();
        acquire_rx.recv().unwrap();
        hold.lock().unwrap();
        locked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    ready_rx.recv().unwrap();
    let mut a = TaskChats::default();
    a.append(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::User, "baseline", None)],
    )
    .unwrap();
    acquire_tx.send(()).unwrap();
    locked_rx.recv().unwrap();
    // The reply below must fail into the pending queue while the peer
    // holds the lock, mirroring the single-flap store hiccup.
    a.remember_response(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::Agent, "stranded reply", None)],
    );
    assert!(a.error.is_some());
    assert_eq!(a.messages["one"].len(), 2);
    assert_eq!(read(slug).unwrap()["one"].len(), 1);
    // Until the lock frees, draining is a quiet no-op that keeps the
    // reply pending and visible in memory.
    a.drain_if_pending(slug);
    assert_eq!(read(slug).unwrap()["one"].len(), 1);
    release_tx.send(()).unwrap();
    peer.join().unwrap();
    a.drain_if_pending(slug);
    assert!(a.error.is_none());
    assert_eq!(read(slug).unwrap()["one"].len(), 2);
    assert!(
        read(slug).unwrap()["one"]
            .iter()
            .any(|m| m.text == "stranded reply")
    );
    // With nothing pending anymore the drain touches the store no further.
    let before = std::fs::read(dir.join("task-conversations.json")).unwrap();
    a.drain_if_pending(slug);
    assert_eq!(
        std::fs::read(dir.join("task-conversations.json")).unwrap(),
        before
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn project_context_keeps_task_identity_and_prioritizes_explicit_task_ids() {
    let mut chats = TaskChats::default();
    chats.messages.insert(
        ".kool-ade-packet/planning/tasks/feature/001-login.md".into(),
        vec![
            ChatMessage::new(ChatRole::User, "Use corporate SSO", None),
            ChatMessage::new(ChatRole::System, "Task reply applied. Resolved.", None),
        ],
    );
    chats.messages.insert(
        "CLR-002".into(),
        vec![ChatMessage::new(ChatRole::User, "Require audit logs", None)],
    );
    let context = chats.project_context("What happened in TASK-001?", 24000);
    for fact in [
        "001-login.md",
        "Use corporate SSO",
        "Resolved.",
        "CLR-002",
        "Require audit logs",
    ] {
        assert!(context.contains(fact));
    }
    assert!(context.find("001-login.md").unwrap() < context.find("CLR-002").unwrap());
}

#[test]
fn refresh_reports_other_window_interactions_once() {
    let dir =
        std::env::temp_dir().join(format!("packet_task_notifications_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut main = TaskChats::default();
    main.ensure_loaded(slug);
    let mut other = TaskChats::default();
    other
        .append(
            slug,
            "CLR-001",
            vec![ChatMessage::new(ChatRole::User, "Use SSO", None)],
        )
        .unwrap();
    main.refresh_now(slug);
    let updates = main.take_updates();
    assert_eq!(updates.len(), 1);
    assert!(updates[0].contains("CLR-001"));
    assert!(updates[0].contains("Use SSO"));
    main.refresh_now(slug);
    assert!(main.take_updates().is_empty());
    assert!(main.project_context("", 24000).contains("Use SSO"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dropping_a_store_flushes_a_reply_queued_by_a_refused_save() {
    // A save refused because a peer held the lock lands in the pending
    // queue; if nothing retries before the store is swapped away (final
    // reply of the session), teardown must still deliver it. Regression:
    // the last reply used to strand in the queue when the app's next
    // retry never ran.
    let dir = std::env::temp_dir().join(format!("packet_task_drop_flush_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lock_path = dir.join("task-conversations.lock");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (acquire_tx, acquire_rx) = std::sync::mpsc::channel::<()>();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let hold = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        ready_tx.send(()).unwrap();
        acquire_rx.recv().unwrap();
        hold.lock().unwrap();
        locked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        drop(hold);
    });
    ready_rx.recv().unwrap();
    acquire_tx.send(()).unwrap();
    locked_rx.recv().unwrap();
    let mut a = TaskChats::default();
    a.remember_response(
        slug,
        "CLR-100",
        vec![ChatMessage::new(
            ChatRole::Agent,
            "Stranded final reply",
            None,
        )],
    );
    assert!(
        a.error.is_some(),
        "save should be refused while the peer holds the lock"
    );
    assert_eq!(a.messages["CLR-100"].len(), 1);
    release_tx.send(()).unwrap();
    peer.join().unwrap();
    drop(a); // teardown delivers the queued reply
    let bytes = std::fs::read_to_string(dir.join("task-conversations.json"))
        .expect("drop-time flush should have written the store");
    assert!(
        bytes.contains("Stranded final reply"),
        "drop-time flush lost the reply: {bytes}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stale_windows_merge_and_failed_response_retries_exactly_once() {
    let dir = std::env::temp_dir().join(format!("packet_task_merge_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut a = TaskChats::default();
    let mut b = TaskChats::default();
    a.ensure_loaded(slug);
    b.ensure_loaded(slug);
    a.append(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::User, "first", None)],
    )
    .unwrap();
    b.append(
        slug,
        "two",
        vec![ChatMessage::new(ChatRole::User, "second", None)],
    )
    .unwrap();
    a.append(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::User, "third", None)],
    )
    .unwrap();
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("task-conversations.lock"))
        .unwrap();
    // Establish the deliberately held lock before testing the nonblocking
    // save path; this setup must not itself depend on scheduling.
    lock.lock().unwrap();
    a.remember_response(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::Agent, "answer", None)],
    );
    assert!(a.error.is_some());
    assert_eq!(a.messages["one"].len(), 3);
    assert_eq!(read(slug).unwrap()["one"].len(), 2);
    drop(lock);
    b.append(
        slug,
        "two",
        vec![ChatMessage::new(ChatRole::Agent, "another answer", None)],
    )
    .unwrap();
    a.retry_save(slug);
    a.retry_save(slug);
    assert!(a.error.is_none());
    let persisted = read(slug).unwrap();
    assert_eq!(persisted["one"].len(), 3);
    assert_eq!(persisted["two"].len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_store_is_never_replaced_by_stale_memory() {
    let dir = std::env::temp_dir().join(format!("packet_task_corrupt_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut chats = TaskChats::default();
    chats
        .append(
            slug,
            "one",
            vec![ChatMessage::new(ChatRole::User, "first", None)],
        )
        .unwrap();
    let path = dir.join("task-conversations.json");
    let previous = std::fs::read(&path).unwrap();
    std::fs::write(&path, "broken").unwrap();
    chats.remember_response(
        slug,
        "one",
        vec![ChatMessage::new(ChatRole::Agent, "answer", None)],
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
    assert_eq!(chats.messages["one"].len(), 2);
    std::fs::write(&path, previous).unwrap();
    chats.retry_save(slug);
    assert!(chats.error.is_none());
    assert_eq!(read(slug).unwrap()["one"].len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn transient_peer_hold_clears_within_budget_so_save_proceeds_instead_of_flapping() {
    let dir = std::env::temp_dir().join(format!("packet_task_settle_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let lock_path = dir.join("task-conversations.lock");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::File::create(&lock_path).unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let hold = std::fs::OpenOptions::new()
            .write(true)
            .open(&lock_path)
            .unwrap();
        ready_tx.send(()).unwrap();
        hold.lock().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        drop(hold);
    });
    ready_rx.recv().unwrap();
    let mut a = TaskChats::default();
    a.append(
        slug,
        "one",
        vec![ChatMessage::new(
            ChatRole::User,
            "arrives mid-peer-hold",
            None,
        )],
    )
    .unwrap();
    peer.join().unwrap();
    assert_eq!(read(slug).unwrap()["one"].len(), 1);
    assert!(a.error.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn independent_streams_survive_restart_without_compaction_or_main_chat() {
    let dir = std::env::temp_dir().join(format!("packet_task_chats_{}", std::process::id()));
    let slug = dir.to_str().unwrap();
    let mut chats = TaskChats::default();
    let first = (0..510)
        .map(|i| ChatMessage::new(ChatRole::User, format!("reply {i}"), None))
        .collect::<Vec<_>>();
    chats.append(slug, "CLR-001", first.clone()).unwrap();
    chats
        .append(
            slug,
            ".kool-ade-packet/planning/tasks/002.md",
            vec![ChatMessage::new(ChatRole::User, "other task", None)],
        )
        .unwrap();
    let mut reopened = TaskChats::default();
    reopened.ensure_loaded(slug);
    assert_eq!(reopened.messages["CLR-001"], first);
    assert_eq!(
        reopened.messages[".kool-ade-packet/planning/tasks/002.md"].len(),
        1
    );
    assert!(!dir.join("chat.jsonl").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
