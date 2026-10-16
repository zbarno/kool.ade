use super::WindowsProcessTree;
use std::{
    fs,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

const MODE_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_MODE";
const STARTED_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_STARTED";
const SURVIVED_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_SURVIVED";
const DESCENDANT_PID_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_DESCENDANT_PID";
const GRANDCHILD_PID_ENV: &str = "KOOLADE_TEST_DISCOVERY_TREE_GRANDCHILD_PID";

// Boundary: Windows discovery's job-object cleanup. This starts a real owner,
// child, and grandchild because mocks cannot prove kernel process-tree cleanup.
// No external dependencies; it uses only the test executable and takes about
// two seconds when successful.
#[test]
fn windows_process_tree_terminates_descendants_when_owner_exits() {
    match std::env::var(MODE_ENV).as_deref() {
        Ok("owner") => run_owner_helper(),
        Ok("descendant") => run_descendant_helper(),
        Ok("grandchild") => run_grandchild_helper(),
        _ => run_process_tree_assertions(),
    }
}

fn run_process_tree_assertions() {
    let directory = TestDirectory::new();
    let started = directory.0.join("grandchild-started");
    let survived = directory.0.join("grandchild-survived");
    let descendant_pid = directory.0.join("descendant-pid");
    let grandchild_pid = directory.0.join("grandchild-pid");
    let mut owner = spawn_test_process("owner");
    owner
        .env(STARTED_ENV, &started)
        .env(SURVIVED_ENV, &survived)
        .env(DESCENDANT_PID_ENV, &descendant_pid)
        .env(GRANDCHILD_PID_ENV, &grandchild_pid)
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);

    let mut child = owner.spawn().expect("spawn suspended test process");
    let tree = WindowsProcessTree::attach(child.id()).expect("assign child to a job object");
    let status = child.wait().expect("wait for owner helper process");
    assert!(status.success(), "owner helper failed: {status}");
    assert!(started.exists(), "the grandchild did not start");

    let descendant = open_process(read_pid(&descendant_pid));
    let grandchild = open_process(read_pid(&grandchild_pid));
    assert_process_in_job(descendant, tree.0, "descendant");
    assert_process_in_job(grandchild, tree.0, "grandchild");
    assert_job_kills_on_close(tree.0);
    assert_job_handle_is_not_inheritable(tree.0);

    drop(tree);
    let descendant_wait = wait_or_terminate(descendant);
    let grandchild_wait = wait_or_terminate(grandchild);
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(descendant);
        windows_sys::Win32::Foundation::CloseHandle(grandchild);
    }
    assert_eq!(
        descendant_wait,
        windows_sys::Win32::Foundation::WAIT_OBJECT_0,
        "descendant survived closing its owner's job object"
    );
    assert_eq!(
        grandchild_wait,
        windows_sys::Win32::Foundation::WAIT_OBJECT_0,
        "grandchild survived closing its owner's job object"
    );
    assert!(
        !survived.exists(),
        "the grandchild wrote its survival marker"
    );
}

fn spawn_test_process(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "windows::tests::windows_process_tree_terminates_descendants_when_owner_exits",
        ])
        .env(MODE_ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn run_owner_helper() {
    let mut descendant = spawn_test_process("descendant")
        .spawn()
        .expect("spawn descendant helper");
    write_pid(DESCENDANT_PID_ENV, descendant.id());
    wait_for_started_marker();
    let _ = descendant.try_wait();
}

fn run_descendant_helper() {
    let mut grandchild = spawn_test_process("grandchild")
        .spawn()
        .expect("spawn grandchild helper");
    write_pid(GRANDCHILD_PID_ENV, grandchild.id());
    wait_for_started_marker();
    std::thread::sleep(Duration::from_secs(30));
    let _ = grandchild.try_wait();
}

fn run_grandchild_helper() {
    let started = std::env::var_os(STARTED_ENV).expect("started marker path");
    let survived = std::env::var_os(SURVIVED_ENV).expect("survival marker path");
    fs::write(started, "started").expect("write grandchild start marker");
    std::thread::sleep(Duration::from_secs(30));
    fs::write(survived, "survived").expect("write grandchild survival marker");
}

fn wait_for_started_marker() {
    let started = std::env::var_os(STARTED_ENV).expect("started marker path");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::path::Path::new(&started).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        std::path::Path::new(&started).exists(),
        "grandchild did not start before the deadline"
    );
}

fn write_pid(path_env: &str, pid: u32) {
    let path = std::env::var_os(path_env).expect("process ID path");
    fs::write(path, pid.to_string()).expect("write process ID");
}

fn read_pid(path: &std::path::Path) -> u32 {
    fs::read_to_string(path)
        .expect("read process ID")
        .parse()
        .expect("parse process ID")
}

fn open_process(pid: u32) -> windows_sys::Win32::Foundation::HANDLE {
    let rights = windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION
        | windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE
        | windows_sys::Win32::System::Threading::PROCESS_TERMINATE;
    let process = unsafe { windows_sys::Win32::System::Threading::OpenProcess(rights, 0, pid) };
    assert!(!process.is_null(), "open process {pid}");
    process
}

fn assert_process_in_job(
    process: windows_sys::Win32::Foundation::HANDLE,
    job: windows_sys::Win32::Foundation::HANDLE,
    name: &str,
) {
    let mut is_in_job = 0;
    let checked = unsafe {
        windows_sys::Win32::System::JobObjects::IsProcessInJob(process, job, &mut is_in_job)
    };
    assert_ne!(checked, 0, "check {name} job membership");
    assert_ne!(
        is_in_job, 0,
        "{name} was not assigned to the production job"
    );
}

fn assert_job_kills_on_close(job: windows_sys::Win32::Foundation::HANDLE) {
    let mut limits =
        windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let queried = unsafe {
        windows_sys::Win32::System::JobObjects::QueryInformationJobObject(
            job,
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
}

fn assert_job_handle_is_not_inheritable(job: windows_sys::Win32::Foundation::HANDLE) {
    let mut handle_flags = 0;
    let checked =
        unsafe { windows_sys::Win32::Foundation::GetHandleInformation(job, &mut handle_flags) };
    assert_ne!(checked, 0, "query production job handle flags");
    assert_eq!(
        handle_flags & windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT,
        0,
        "production job handle must not be inherited"
    );
}

fn wait_or_terminate(
    process: windows_sys::Win32::Foundation::HANDLE,
) -> windows_sys::Win32::Foundation::WAIT_EVENT {
    let wait = unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(process, 5000) };
    if wait != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
        unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(process, 1);
            windows_sys::Win32::System::Threading::WaitForSingleObject(process, 5000);
        }
    }
    wait
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
