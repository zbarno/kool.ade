use super::*;

#[test]
fn project_state_gate_uses_private_storage_without_a_git_checkout() {
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
    first.unlock().unwrap();
    assert!(second.try_lock().is_ok());
}
