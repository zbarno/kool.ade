use std::{
    io,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED, WaitForSingleObject},
};

use super::WindowsVmProcessJob;
use crate::computer_use::windows_backend::WindowsVmJobLimits;

pub(super) fn spawn_contained(
    command: Command,
    limits: WindowsVmJobLimits,
) -> io::Result<WindowsVmProcessJob> {
    spawn_inner(command, limits, &[])
}

#[cfg(test)]
pub(in crate::computer_use::windows_backend) fn spawn_contained_with_test_socket_path(
    command: Command,
    limits: WindowsVmJobLimits,
    socket_path: &std::path::Path,
) -> io::Result<WindowsVmProcessJob> {
    let test_env = [(
        std::ffi::OsString::from("KOOLADE_TEST_WINDOWS_AF_UNIX_CHILD_SOCKET"),
        socket_path.as_os_str().to_owned(),
    )];
    spawn_inner(command, limits, &test_env)
}

fn spawn_inner(
    mut command: Command,
    limits: WindowsVmJobLimits,
    additional_env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> io::Result<WindowsVmProcessJob> {
    let program = std::path::Path::new(command.get_program());
    if !program.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "QEMU executable path must be absolute",
        ));
    }
    let job = super::create_configured_job(limits)?;
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
    if !std::path::Path::new(&system_root).is_absolute() {
        return Err(io::Error::other("SystemRoot is not an absolute path"));
    }
    command.env_clear();
    command.env("SystemRoot", &system_root);
    command.env("WINDIR", &system_root);
    // Production passes no extra environment. The test-only wrapper supplies
    // only the path needed by its current-executable AF_UNIX child fixture.
    for (key, value) in additional_env {
        command.env(key, value);
    }
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let mut child = command.spawn()?;
    let process = child.as_raw_handle() as HANDLE;
    if let Err(error) = super::identity::verify_current_user_owner(process)
        .and_then(|()| super::assign_job(job.0, process))
        .and_then(|()| super::verify_job_membership(job.0, process))
        .and_then(|()| super::resume_primary_thread(child.id()))
    {
        // Until assignment succeeds, explicitly terminate the suspended
        // process. After assignment, closing the job is also fail-closed.
        return terminate_child_bounded(&mut child).and(Err(error));
    }
    Ok(WindowsVmProcessJob { job, child })
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
            std::time::Duration::from_secs(5).as_millis() as u32,
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
