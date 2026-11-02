use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, ReadFile, WriteFile},
    System::{
        Pipes::{GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE, SetNamedPipeHandleState},
        Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
};

use crate::execution::docker::engine::endpoint::named_pipe::image::signature;

use super::super::HelperOutcome;
use super::super::protocol::{self, HandoffResultV1, HandoffV1};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct HandoffChannel(HANDLE);

impl HandoffChannel {
    pub(super) fn receive_request(&mut self) -> anyhow::Result<HandoffV1> {
        let mut buffer = vec![0_u8; protocol::MAX_HANDOFF_BYTES];
        let mut read = 0_u32;
        let succeeded = unsafe {
            ReadFile(
                self.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        anyhow::ensure!(
            succeeded != 0,
            "Windows setup approval handoff could not be read"
        );
        buffer.truncate(read as usize);
        protocol::decode(&buffer)
    }

    pub(super) fn send_result(
        &mut self,
        ticket_id: &str,
        outcome: HelperOutcome,
    ) -> anyhow::Result<()> {
        let exit_code = match outcome {
            HelperOutcome::Completed => 0,
            HelperOutcome::RebootRequired => 3010,
            HelperOutcome::Failed => 1,
        };
        let message = HandoffResultV1 {
            schema_version: 1,
            ticket_id: ticket_id.to_owned(),
            outcome,
            exit_code,
        };
        let bytes = protocol::encode(&message)?;
        let mut written = 0_u32;
        anyhow::ensure!(
            unsafe {
                WriteFile(
                    self.0,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } != 0
                && written as usize == bytes.len(),
            "Windows setup result could not be returned to Kool.ad/e"
        );
        Ok(())
    }
}

impl Drop for HandoffChannel {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn connect(
    pipe_id: uuid::Uuid,
    expected_publisher: &str,
) -> anyhow::Result<HandoffChannel> {
    let pipe_name = format!(r"\\.\pipe\KooladeWindowsSetup-{pipe_id}");
    let wide_name = pipe_name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let handle = loop {
        let handle = unsafe {
            CreateFileW(
                wide_name.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break handle;
        }
        let error = unsafe { GetLastError() };
        anyhow::ensure!(
            error == ERROR_FILE_NOT_FOUND
                || error == windows_sys::Win32::Foundation::ERROR_PIPE_BUSY,
            "Windows setup handoff pipe could not be opened"
        );
        anyhow::ensure!(Instant::now() < deadline, "Windows setup handoff timed out");
        std::thread::sleep(Duration::from_millis(50));
    };
    let channel = HandoffChannel(handle);
    let mut mode = PIPE_READMODE_MESSAGE;
    anyhow::ensure!(
        unsafe { SetNamedPipeHandleState(handle, &mut mode, std::ptr::null(), std::ptr::null(),) }
            != 0,
        "Windows setup handoff pipe mode is invalid"
    );
    verify_server_publisher(handle, expected_publisher)?;
    Ok(channel)
}

fn verify_server_publisher(pipe: HANDLE, expected: &str) -> anyhow::Result<()> {
    let mut server_pid = 0_u32;
    anyhow::ensure!(
        unsafe { GetNamedPipeServerProcessId(pipe, &mut server_pid) } != 0 && server_pid > 0,
        "Windows setup handoff server identity is unavailable"
    );
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, server_pid) };
    anyhow::ensure!(
        !process.is_null(),
        "Windows setup handoff server is unavailable"
    );
    let result = process_image_publisher(process, expected);
    unsafe { CloseHandle(process) };
    result
}

fn process_image_publisher(process: HANDLE, expected: &str) -> anyhow::Result<()> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    anyhow::ensure!(
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut length,
            )
        } != 0
            && length > 0
            && (length as usize) < buffer.len(),
        "Windows setup handoff server image is unavailable"
    );
    let path = std::path::PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    let path = path.canonicalize()?;
    anyhow::ensure!(
        signature::authenticode_publisher(&path).as_deref() == Some(expected),
        "Windows setup handoff server publisher is not trusted"
    );
    Ok(())
}

pub(super) fn now_unix_ms() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64)
}
