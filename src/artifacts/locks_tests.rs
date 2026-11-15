use super::*;

#[test]
// Production boundary: kernel-backed file-lock ownership through the real
// project-state gate. A file-presence assertion cannot prove contention.
// Dependencies: local temporary filesystem only; setup cost is low.
fn project_state_gate_blocks_a_second_owner_while_the_first_handle_is_live() {
    let project = tempfile::tempdir().unwrap();
    let private_root = tempfile::tempdir().unwrap();
    let first = lock(private_root.path(), project.path()).unwrap();
    let path = lock_path(private_root.path(), project.path()).unwrap();
    let second = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();

    assert!(second.try_lock().is_err());
    drop(first);
    assert!(second.try_lock().is_ok());
}

#[test]
// Production boundary: OS lock release when an abandoned owner's handle
// closes, while its empty lock file remains. This cannot be proven by a
// lock-path unit test because ownership is maintained by the operating system.
// Dependencies: local temporary filesystem only; setup cost is low.
fn abandoned_zero_byte_project_state_lock_file_does_not_block_recovery() {
    let project = tempfile::tempdir().unwrap();
    let private_root = tempfile::tempdir().unwrap();
    let first = lock(private_root.path(), project.path()).unwrap();
    let path = lock_path(private_root.path(), project.path()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    drop(first);

    assert_eq!(
        fs::metadata(&path).unwrap().len(),
        0,
        "the abandoned lock file remains present and empty"
    );
    let recovered = lock(private_root.path(), project.path())
        .expect("OS lock ownership is released when the abandoned handle closes");
    let second = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    assert!(
        second.try_lock().is_err(),
        "the recovered owner now holds it"
    );
    drop(recovered);
}
