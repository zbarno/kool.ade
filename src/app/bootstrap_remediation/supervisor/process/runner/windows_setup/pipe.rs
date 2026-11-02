use std::time::{Duration, Instant};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, GetLastError,
        HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile},
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
            PIPE_NOWAIT, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        },
        Threading::{TerminateProcess, WaitForSingleObject},
    },
};

use crate::{
    app::bootstrap_remediation::supervisor::{Cancellation, ProcessRunFailure},
    windows_setup_helper::protocol,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(super) struct Pipe(HANDLE);

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            DisconnectNamedPipe(self.0);
            CloseHandle(self.0);
        }
    }
}

pub(super) fn create(pipe_id: uuid::Uuid) -> anyhow::Result<Pipe> {
    let name = format!(r"\\.\pipe\KooladeWindowsSetup-{pipe_id}");
    let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mode = PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            mode,
            1,
            protocol::MAX_HANDOFF_BYTES as u32,
            protocol::MAX_HANDOFF_BYTES as u32,
            0,
            std::ptr::null(),
        )
    };
    anyhow::ensure!(
        pipe != INVALID_HANDLE_VALUE,
        "could not create the private setup pipe"
    );
    Ok(Pipe(pipe))
}

pub(super) fn connect(
    pipe: &Pipe,
    process: HANDLE,
    expected_process_id: u32,
    cancellation: &Cancellation,
) -> Result<(), ProcessRunFailure> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if cancellation.is_cancelled() {
            terminate_helper(process);
            return Err(ProcessRunFailure::Cancelled);
        }
        if Instant::now() >= deadline || process_exited(process)? {
            terminate_helper(process);
            return Err(ProcessRunFailure::TimedOut);
        }
        if unsafe { ConnectNamedPipe(pipe.0, std::ptr::null_mut()) } != 0 {
            break;
        }
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => break,
            ERROR_PIPE_LISTENING | ERROR_NO_DATA => std::thread::sleep(Duration::from_millis(50)),
            _ => return Err(ProcessRunFailure::FailedAfterLaunch),
        }
    }
    let mut client = 0_u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.0, &mut client) } == 0
        || client != expected_process_id
    {
        terminate_helper(process);
        return Err(ProcessRunFailure::FailedAfterLaunch);
    }
    Ok(())
}

pub(super) fn read_message(
    pipe: &Pipe,
    process: HANDLE,
    cancellation: &Cancellation,
) -> Result<Vec<u8>, ProcessRunFailure> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        if cancellation.is_cancelled() {
            terminate_helper(process);
            return Err(ProcessRunFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            terminate_helper(process);
            return Err(ProcessRunFailure::TimedOut);
        }
        if process_exited(process)? {
            return Err(ProcessRunFailure::FailedAfterLaunch);
        }
        let mut buffer = vec![0_u8; protocol::MAX_HANDOFF_BYTES];
        let mut read = 0_u32;
        let succeeded = unsafe {
            ReadFile(
                pipe.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if succeeded != 0 {
            buffer.truncate(read as usize);
            return Ok(buffer);
        }
        match unsafe { GetLastError() } {
            ERROR_NO_DATA => std::thread::sleep(Duration::from_millis(50)),
            _ => return Err(ProcessRunFailure::FailedAfterLaunch),
        }
    }
}

pub(super) fn write_message(pipe: &Pipe, bytes: &[u8]) -> Result<(), ProcessRunFailure> {
    let mut written = 0_u32;
    if unsafe {
        WriteFile(
            pipe.0,
            bytes.as_ptr(),
            bytes.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    } == 0
        || written as usize != bytes.len()
    {
        return Err(ProcessRunFailure::FailedAfterLaunch);
    }
    unsafe { FlushFileBuffers(pipe.0) };
    Ok(())
}

fn process_exited(process: HANDLE) -> Result<bool, ProcessRunFailure> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(ProcessRunFailure::FailedAfterLaunch),
    }
}

fn terminate_helper(process: HANDLE) {
    unsafe {
        TerminateProcess(process, 1);
        WaitForSingleObject(process, 5_000);
    }
}
