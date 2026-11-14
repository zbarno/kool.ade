#![allow(dead_code)] // Consumed by the Windows WHPX backend integration seam.

use std::{
    io,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, ExitStatus},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32},
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
            JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME, WaitForSingleObject},
    },
};

use self::identity::verify_current_user_owner;
use super::WindowsVmJobLimits;

mod identity;

const MAX_JOB_CONFIG_BYTES: u32 = 1024;

struct JobHandle(HANDLE);

// Kernel job handles are process-wide capabilities and the Win32 APIs used
// below permit moving this owned handle between threads.
unsafe impl Send for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // The job is unnamed, created with the current token's default DACL,
        // never inherited, and carries KILL_ON_JOB_CLOSE.
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct WindowsVmProcessJob {
    job: JobHandle,
    child: Child,
}

impl WindowsVmProcessJob {
    /// Starts a trusted, absolute-path QEMU command suspended, places it in a
    /// private resource-limited Job Object, verifies ownership, then resumes it.
    /// Any setup failure kills the still-suspended child and returns an error.
    pub(crate) fn spawn(command: Command, limits: WindowsVmJobLimits) -> io::Result<Self> {
        let readiness = super::probe_windows_wpx();
        if !readiness.ready_for_adapter || !readiness.release_enabled {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "WHPX backend is fail-closed: ready={}, release_enabled={}, blocker={:?}",
                    readiness.ready_for_adapter, readiness.release_enabled, readiness.blocker,
                ),
            ));
        }
        Self::spawn_contained(command, limits)
    }

    fn spawn_contained(mut command: Command, limits: WindowsVmJobLimits) -> io::Result<Self> {
        let program = std::path::Path::new(command.get_program());
        if !program.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "QEMU executable path must be absolute",
            ));
        }
        let job = create_configured_job(limits)?;
        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
        if !std::path::Path::new(&system_root).is_absolute() {
            return Err(io::Error::other("SystemRoot is not an absolute path"));
        }
        command.env_clear();
        command.env("SystemRoot", &system_root);
        command.env("WINDIR", &system_root);
        command.creation_flags(
            windows_sys::Win32::System::Threading::CREATE_SUSPENDED
                | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
        );
        let mut child = command.spawn()?;
        let process = child.as_raw_handle() as HANDLE;
        if let Err(error) = verify_current_user_owner(process)
            .and_then(|()| assign_job(job.0, process))
            .and_then(|()| verify_job_membership(job.0, process))
            .and_then(|()| resume_primary_thread(child.id()))
        {
            // Until assignment succeeds, explicitly terminate the suspended
            // process. After assignment, closing the job is also fail-closed.
            return terminate_child_bounded(&mut child).and(Err(error));
        }
        Ok(Self { job, child })
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) fn terminate(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Job Object stop timeout must be non-zero",
            ));
        }
        if unsafe { TerminateJobObject(self.job.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        let wait = unsafe { WaitForSingleObject(self.child.as_raw_handle() as HANDLE, timeout_ms) };
        if wait != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
            return Err(if wait == windows_sys::Win32::Foundation::WAIT_TIMEOUT {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Job Object stop exceeded its deadline",
                )
            } else {
                io::Error::last_os_error()
            });
        }
        self.child.wait()
    }
}

fn terminate_child_bounded(child: &mut Child) -> io::Result<()> {
    match child.kill() {
        Ok(()) => {}
        Err(_error) if child.try_wait()?.is_some() => return child.wait().map(drop),
        Err(error) => return Err(error),
    }
    let wait = unsafe {
        WaitForSingleObject(
            child.as_raw_handle() as HANDLE,
            Duration::from_secs(5).as_millis() as u32,
        )
    };
    if wait != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
        return Err(if wait == windows_sys::Win32::Foundation::WAIT_TIMEOUT {
            io::Error::new(io::ErrorKind::TimedOut, "suspended QEMU did not terminate")
        } else {
            io::Error::last_os_error()
        });
    }
    child.wait().map(drop)
}

fn create_configured_job(limits: WindowsVmJobLimits) -> io::Result<JobHandle> {
    use std::ptr::null;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    };

    // NULL name makes the object private to this process tree; the default
    // security descriptor comes from the current user's token.
    let handle = unsafe { CreateJobObjectW(null(), null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = JobHandle(handle);
    let mut extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    extended.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    extended.BasicLimitInformation.ActiveProcessLimit = limits.active_processes;
    extended.ProcessMemoryLimit = limits.process_memory_bytes;
    set_job_info(
        job.0,
        JobObjectExtendedLimitInformation,
        &extended,
        std::mem::size_of_val(&extended),
    )?;

    let cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
        ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
            CpuRate: limits.cpu_rate_hundredths_percent,
        },
    };
    set_job_info(
        job.0,
        JobObjectCpuRateControlInformation,
        &cpu,
        std::mem::size_of_val(&cpu),
    )?;
    Ok(job)
}

fn set_job_info<T>(job: HANDLE, class: i32, info: &T, size: usize) -> io::Result<()> {
    if size > MAX_JOB_CONFIG_BYTES as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Job Object configuration exceeded its bound",
        ));
    }
    // SAFETY: `info` points to the exact structure for the supplied class.
    let result =
        unsafe { SetInformationJobObject(job, class, (info as *const T).cast(), size as u32) };
    (result != 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

fn assign_job(job: HANDLE, process: HANDLE) -> io::Result<()> {
    // SAFETY: both handles are owned and live through this call.
    (unsafe { AssignProcessToJobObject(job, process) } != 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

fn verify_job_membership(job: HANDLE, process: HANDLE) -> io::Result<()> {
    let mut in_job = 0;
    // SAFETY: `in_job` is writable and both handles remain live.
    if unsafe { IsProcessInJob(process, job, &mut in_job) } != 0 && in_job != 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "QEMU is not contained by its Job Object",
        ))
    }
}

fn resume_primary_thread(pid: u32) -> io::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{Thread32First, Thread32Next};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut found = unsafe { Thread32First(snapshot, &mut entry) };
    let mut primary = None;
    while found != 0 {
        if entry.th32OwnerProcessID == pid {
            if primary.replace(entry.th32ThreadID).is_some() {
                unsafe { CloseHandle(snapshot) };
                return Err(io::Error::other(
                    "suspended QEMU process has multiple threads before containment",
                ));
            }
        }
        found = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    let thread_id =
        primary.ok_or_else(|| io::Error::other("QEMU primary thread is unavailable"))?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(io::Error::last_os_error());
    }
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe { CloseHandle(thread) };
    if previous_suspend_count != 1 {
        return Err(io::Error::other(
            "QEMU primary thread was not suspended exactly once",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
