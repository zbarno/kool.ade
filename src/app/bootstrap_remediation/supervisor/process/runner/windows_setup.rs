use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    time::{Duration, Instant},
};

mod pipe;

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    System::Threading::{GetExitCodeProcess, GetProcessId, TerminateProcess, WaitForSingleObject},
    UI::{
        Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

use crate::{
    app::bootstrap_remediation::supervisor::{
        Cancellation, ElevationReceipt, ProcessOutput, ProcessRunError, ProcessRunFailure,
        TrustedInvocation, TrustedProgram,
    },
    windows_setup_helper::{
        HelperOutcome,
        protocol::{self, HandoffResultV1, HandoffV1, WSL_FEATURE_OPERATION, WSL_FEATURE_TASK},
    },
};

use pipe::Pipe;

const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub(super) fn run(
    invocation: TrustedInvocation,
    elevation: ElevationReceipt,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    if invocation.host() != crate::app::bootstrap_repair_policy::Host::Windows
        || invocation.program() != TrustedProgram::WindowsWslFeatures
        || invocation.timeout() > MAX_OPERATION_TIMEOUT
        || cancellation.is_cancelled()
    {
        return Err(ProcessRunError::not_started(
            if cancellation.is_cancelled() {
                ProcessRunFailure::Cancelled
            } else {
                ProcessRunFailure::FailedBeforeLaunch
            },
        ));
    }
    let authorization = elevation
        .setup_authorization
        .as_ref()
        .ok_or_else(|| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    if authorization.expires_at_unix_ms <= now_unix_ms().unwrap_or(u64::MAX) {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }

    let ticket_id = uuid::Uuid::new_v4();
    let request = HandoffV1 {
        schema_version: 1,
        ticket_id: ticket_id.to_string(),
        operation: WSL_FEATURE_OPERATION.into(),
        task_id: authorization.task_id.clone(),
        operation_id: authorization.operation_id.clone(),
        owner_instance_id: authorization.owner_instance_id.clone(),
        receipt_id: authorization.receipt_id.clone(),
        scope_digest: authorization.scope_digest.clone(),
        plan_digest: elevation.plan_digest.clone(),
        invocation_digest: elevation.invocation_digest.clone(),
        expires_at_unix_ms: authorization.expires_at_unix_ms,
    };
    request
        .validate(now_unix_ms().unwrap_or(u64::MAX))
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let pipe_id = uuid::Uuid::new_v4();
    let pipe = pipe::create(pipe_id)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let process =
        launch_helper(pipe_id).map_err(|failure| ProcessRunError::not_started(failure))?;

    let run = communicate(pipe, process, request, cancellation);
    run.map_err(ProcessRunError::after_launch)
}

fn launch_helper(pipe_id: uuid::Uuid) -> Result<Handle, ProcessRunFailure> {
    let executable = std::env::current_exe().map_err(|_| ProcessRunFailure::FailedBeforeLaunch)?;
    let executable = wide_os(executable.as_os_str());
    let parameters = wide(&format!(
        "--windows-setup-helper enable-wsl-features --handoff-pipe {pipe_id}"
    ));
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = executable.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(if unsafe { GetLastError() } == ERROR_CANCELLED {
            ProcessRunFailure::Cancelled
        } else {
            ProcessRunFailure::FailedBeforeLaunch
        });
    }
    if info.hProcess.is_null() {
        return Err(ProcessRunFailure::FailedBeforeLaunch);
    }
    Ok(Handle(info.hProcess))
}

fn communicate(
    pipe: Pipe,
    process: Handle,
    request: HandoffV1,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunFailure> {
    let process_id = unsafe { GetProcessId(process.0) };
    if process_id == 0 {
        return Err(ProcessRunFailure::FailedAfterLaunch);
    }
    pipe::connect(&pipe, process.0, process_id, cancellation)?;
    let bytes = protocol::encode(&request).map_err(|_| ProcessRunFailure::FailedAfterLaunch)?;
    pipe::write_message(&pipe, &bytes)?;
    let response_bytes = pipe::read_message(&pipe, process.0, cancellation)?;
    let response: HandoffResultV1 =
        protocol::decode(&response_bytes).map_err(|_| ProcessRunFailure::FailedAfterLaunch)?;
    response
        .validate(&request.ticket_id)
        .map_err(|_| ProcessRunFailure::FailedAfterLaunch)?;
    wait_for_helper(process.0, cancellation)?;
    let helper_exit = process_exit_code(process.0)?;
    if helper_exit != response.exit_code as u32 {
        return Err(ProcessRunFailure::FailedAfterLaunch);
    }
    let _typed_outcome: HelperOutcome = response.outcome;
    Ok(ProcessOutput {
        exit_code: response.exit_code,
        stdout: String::new(),
        stderr: if response.outcome == HelperOutcome::Failed {
            "Windows setup helper reported an operation failure.".into()
        } else {
            String::new()
        },
        stdout_truncated: false,
        stderr_truncated: false,
    })
}

fn wait_for_helper(process: HANDLE, cancellation: &Cancellation) -> Result<(), ProcessRunFailure> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if cancellation.is_cancelled() {
            terminate_helper(process);
            return Err(ProcessRunFailure::Cancelled);
        }
        match unsafe { WaitForSingleObject(process, 50) } {
            WAIT_OBJECT_0 => return Ok(()),
            windows_sys::Win32::Foundation::WAIT_TIMEOUT if Instant::now() < deadline => {}
            _ => {
                terminate_helper(process);
                return Err(ProcessRunFailure::TimedOut);
            }
        }
    }
}

fn process_exit_code(process: HANDLE) -> Result<u32, ProcessRunFailure> {
    let mut code = 0_u32;
    if unsafe { GetExitCodeProcess(process, &mut code) } == 0 {
        return Err(ProcessRunFailure::FailedAfterLaunch);
    }
    Ok(code)
}

fn terminate_helper(process: HANDLE) {
    unsafe {
        TerminateProcess(process, 1);
        WaitForSingleObject(process, 5_000);
    }
}

fn now_unix_ms() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
