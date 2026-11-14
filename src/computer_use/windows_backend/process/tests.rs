use super::*;
use std::time::Duration;
use windows_sys::Win32::{
    JobObjects::{
        IsProcessInJob, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    },
    System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
};

/// Production boundary: Windows process creation to Job Object assignment
/// before QEMU's first instruction. A pure unit test cannot prove kernel
/// assignment, token ownership, CPU/memory limit acceptance, or kill-on-close.
/// Dependency: native Windows process APIs and System32 `timeout.exe`; setup < 1s.
#[test]
fn suspended_child_is_owned_limited_and_stops_with_its_job() {
    let root = std::env::var_os("SystemRoot").expect("SystemRoot");
    let timeout = std::path::PathBuf::from(root)
        .join("System32")
        .join("timeout.exe");
    let limits = super::super::WindowsVmJobLimits::new(512 * 1024 * 1024, 1_000, 1)
        .expect("bounded job limits");
    let mut process = WindowsVmProcessJob::spawn_contained(
        {
            let mut command = Command::new(timeout);
            command.args(["/t", "20", "/nobreak"]);
            command
        },
        limits,
    )
    .expect("assign QEMU-like child before resume");
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process.id()) };
    assert!(!handle.is_null());
    let mut is_in_job = 0;
    assert!(unsafe { IsProcessInJob(handle, process.job.0, &mut is_in_job) } != 0);
    unsafe { CloseHandle(handle) };
    assert_ne!(is_in_job, 0);

    let mut extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut returned = 0_u32;
    assert!(
        unsafe {
            QueryInformationJobObject(
                process.job.0,
                JobObjectExtendedLimitInformation,
                (&mut extended as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&extended) as u32,
                &mut returned,
            )
        } != 0
    );
    assert_eq!(extended.ProcessMemoryLimit, 512 * 1024 * 1024);
    assert_eq!(extended.BasicLimitInformation.ActiveProcessLimit, 1);
    assert_eq!(
        extended.BasicLimitInformation.LimitFlags
            & (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS),
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    );
    let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    assert!(
        unsafe {
            QueryInformationJobObject(
                process.job.0,
                JobObjectCpuRateControlInformation,
                (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                std::mem::size_of_val(&cpu) as u32,
                &mut returned,
            )
        } != 0
    );
    assert_ne!(cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, 0);
    assert_eq!(unsafe { cpu.Anonymous.CpuRate }, 1_000);

    process
        .terminate(Duration::from_secs(5))
        .expect("terminate contained process tree");
    assert!(process.try_wait().unwrap().is_some());
}
