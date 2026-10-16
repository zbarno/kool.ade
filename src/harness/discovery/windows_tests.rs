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
const PID_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_PID";

// Boundary: Windows discovery's job-object cleanup. A real child and grandchild
// are needed because a mock cannot prove kernel process-tree termination. This
// uses Windows system executables and takes about six seconds.
#[test]
fn windows_process_tree_terminates_descendants_when_owner_exits() {
    if let (Some(started), Some(survived), Some(pid)) = (
        std::env::var_os(STARTED_ENV),
        std::env::var_os(SURVIVED_ENV),
        std::env::var_os(PID_ENV),
    ) {
        descendant_helper(started.into(), survived.into(), pid.into());
        return;
    }

    let directory = TestDirectory::new();
    let started = directory.0.join("descendant-started");
    let survived = directory.0.join("descendant-survived");
    let pid = directory.0.join("descendant-pid");
    let mut owner = Command::new(std::env::current_exe().expect("test executable"));
    owner
        .args([
            "--exact",
            "windows::tests::windows_process_tree_terminates_descendants_when_owner_exits",
            "--nocapture",
        ])
        .env(STARTED_ENV, &started)
        .env(SURVIVED_ENV, &survived)
        .env(PID_ENV, &pid)
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
    let descendant_pid = fs::read_to_string(&pid)
        .expect("read descendant process ID")
        .parse()
        .expect("parse descendant process ID");
    let process = unsafe {
        windows_sys::Win32::System::Threading::OpenProcess(
            windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            descendant_pid,
        )
    };
    assert!(!process.is_null(), "open descendant process for job check");
    let mut is_in_job = 0;
    let checked = unsafe {
        windows_sys::Win32::System::JobObjects::IsProcessInJob(process, tree.0, &mut is_in_job)
    };
    unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
    assert_ne!(checked, 0, "check descendant job membership");
    assert_ne!(
        is_in_job, 0,
        "descendant was not assigned to the production job object"
    );
    let mut limits =
        windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let queried = unsafe {
        windows_sys::Win32::System::JobObjects::QueryInformationJobObject(
            tree.0,
            windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation,
            (&mut limits as *mut windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION)
                .cast::<std::ffi::c_void>(),
            std::mem::size_of_val(&limits) as u32,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(queried, 0, "query production job limits");
    assert_ne!(
        limits.BasicLimitInformation.LimitFlags
            & windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        0,
        "production job lacks kill-on-close limit"
    );
    let mut handle_flags = 0;
    let handle_checked =
        unsafe { windows_sys::Win32::Foundation::GetHandleInformation(tree.0, &mut handle_flags) };
    assert_ne!(handle_checked, 0, "query production job handle flags");
    assert_eq!(
        handle_flags & windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT,
        0,
        "production job handle must not be inherited"
    );

    drop(tree);
    std::thread::sleep(Duration::from_millis(6500));
    assert!(
        !survived.exists(),
        "the descendant survived closing its owner's job object"
    );
}

fn descendant_helper(
    started: std::path::PathBuf,
    survived: std::path::PathBuf,
    pid: std::path::PathBuf,
) {
    let command_processor = std::path::Path::new(
        &std::env::var_os("SystemRoot").expect("Windows system root is available"),
    )
    .join("System32")
    .join("cmd.exe");
    let script = format!(
        "echo started>\"{}\" & ping -n 7 127.0.0.1 >nul & echo leaked>\"{}\"",
        started.display(),
        survived.display()
    );
    let mut descendant = Command::new(command_processor)
        .arg("/C")
        .raw_arg(format!(" {script}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn descendant process");
    fs::write(pid, descendant.id().to_string()).expect("record descendant process ID");

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
