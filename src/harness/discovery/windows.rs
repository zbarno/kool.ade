pub(crate) struct WindowsProcessTree(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsProcessTree {
    pub(crate) fn attach(pid: u32) -> std::io::Result<Self> {
        use std::mem::size_of;
        use std::ptr::null;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                    SetInformationJobObject,
                },
                Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
            },
        };

        let job = unsafe { CreateJobObjectW(null(), null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            unsafe { CloseHandle(job) };
            return Err(std::io::Error::last_os_error());
        }
        let process: HANDLE = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            unsafe { CloseHandle(job) };
            return Err(std::io::Error::last_os_error());
        }
        let assigned = unsafe { AssignProcessToJobObject(job, process) };
        unsafe { CloseHandle(process) };
        if assigned == 0 {
            unsafe { CloseHandle(job) };
            return Err(std::io::Error::last_os_error());
        }
        if let Err(error) = resume_primary_thread(pid) {
            unsafe { CloseHandle(job) };
            return Err(error);
        }
        Ok(Self(job))
    }
}

#[cfg(windows)]
fn resume_primary_thread(pid: u32) -> std::io::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut primary_thread = None;
    let mut found = unsafe { Thread32First(snapshot, &mut entry) };
    while found != 0 {
        if entry.th32OwnerProcessID == pid && primary_thread.replace(entry.th32ThreadID).is_some() {
            unsafe { CloseHandle(snapshot) };
            return Err(std::io::Error::other(
                "suspended provider has an unexpected thread count",
            ));
        }
        found = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    let thread_id = primary_thread.ok_or_else(|| {
        std::io::Error::other("suspended provider's primary thread could not be found")
    })?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let resumed = unsafe { ResumeThread(thread) };
    unsafe { CloseHandle(thread) };
    if resumed == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
impl Drop for WindowsProcessTree {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(all(test, windows))]
#[path = "windows_tests.rs"]
mod tests;
