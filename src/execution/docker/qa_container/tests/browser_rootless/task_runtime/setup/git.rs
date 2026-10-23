use std::{fs, process::Command};

pub(super) fn copy_app_fixture(task_clone: &std::path::Path) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/python_task_web");
    for relative in ["build.py", "test_app.py", "src/server.py", "src/index.html"] {
        let source = fixture.join(relative);
        let destination = task_clone.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}

pub(super) fn initialize_git(root: &std::path::Path) {
    let output = Command::new("git")
        .args(["init", "--quiet", "--initial-branch=main"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "initialize synthetic task clone");
}

pub(super) fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn git_commit(root: &std::path::Path, message: &str) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Koolade Test Fixture",
            "-c",
            "user.email=koolade-test-fixture@users.noreply.github.com",
            "commit",
            "--quiet",
            "--message",
            message,
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic fixture commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn git_text(root: &std::path::Path, args: &[&str]) -> String {
    String::from_utf8(git_output(root, args))
        .unwrap()
        .trim()
        .to_owned()
}

pub(super) fn git_output(root: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
