use super::WindowsProcessTree;
use std::{
    fs,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

const STARTED_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_STARTED";
const SURVIVED_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_SURVIVED";

// Boundary: Windows discovery's job-object cleanup. A real child and grandchild
// are needed because a mock cannot prove kernel process-tree termination. This
// uses only Windows system executables and takes about four seconds.
#[test]
fn windows_process_tree_terminates_descendants_when_owner_exits() {
    if let (Some(started), Some(survived)) = (
        std::env::var_os(STARTED_ENV),
        std::env::var_os(SURVIVED_ENV),
    ) {
        descendant_helper(started.into(), survived.into());
        return;
    }

    let directory = TestDirectory::new();
    let started = directory.0.join("descendant-started");
    let survived = directory.0.join("descendant-survived");
    let mut owner = Command::new(std::env::current_exe().expect("test executable"));
    owner
        .args([
            "--exact",
            "windows::tests::windows_process_tree_terminates_descendants_when_owner_exits",
            "--nocapture",
        ])
        .env(STARTED_ENV, &started)
        .env(SURVIVED_ENV, &survived)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);

    let child = owner.spawn().expect("spawn suspended test process");
    let tree = WindowsProcessTree::attach(child.id()).expect("assign child to a job object");
    let output = child.wait_with_output().expect("wait for helper process");
    assert!(
        output.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "the child test harness did not run the requested helper"
    );
    assert!(
        started.exists(),
        "the descendant started before its owner exited"
    );

    drop(tree);
    std::thread::sleep(Duration::from_millis(3500));
    assert!(
        !survived.exists(),
        "the descendant survived closing its owner's job object"
    );
}

fn descendant_helper(started: std::path::PathBuf, survived: std::path::PathBuf) {
    let command_processor = std::path::Path::new(
        &std::env::var_os("SystemRoot").expect("Windows system root is available"),
    )
    .join("System32")
    .join("cmd.exe");
    let script = format!(
        "echo started>\"{}\" & ping -n 4 127.0.0.1 >nul & echo leaked>\"{}\"",
        started.display(),
        survived.display()
    );
    let mut descendant = Command::new(command_processor)
        .arg("/C")
        .raw_arg(format!(" {script}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn descendant process");

    let deadline = Instant::now() + Duration::from_secs(2);
    while !started.exists() && Instant::now() < deadline {
        if let Some(status) = descendant
            .try_wait()
            .expect("query descendant process status")
        {
            let output = descendant
                .wait_with_output()
                .expect("collect failed descendant output");
            panic!(
                "descendant exited before creating its start marker ({status}); stdout: {}; stderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(started.exists(), "descendant did not start");
    assert!(
        descendant
            .try_wait()
            .expect("query descendant process status")
            .is_none(),
        "descendant exited before its owner was closed"
    );
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "koolade-windows-probe-tree-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create private test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
