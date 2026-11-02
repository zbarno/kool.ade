use std::{
    os::windows::{ffi::OsStringExt, process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::CloseHandle,
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::{
        SystemInformation::GetSystemDirectoryW,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

use super::{HelperOutcome, Operation, dism_outcome};

mod handoff;
mod host;

const MAX_OPERATION_TIME: Duration = Duration::from_secs(5 * 60);

pub(super) fn run(operation: Operation, pipe_id: uuid::Uuid) -> anyhow::Result<HelperOutcome> {
    verify_release_identity()?;
    anyhow::ensure!(
        is_elevated()?,
        "Windows setup helper requires an administrator token"
    );
    let mut channel = handoff::connect(pipe_id, super::configured_publisher()?)?;
    let request = channel.receive_request()?;
    request.validate(handoff::now_unix_ms()?)?;
    host::validate_supported_windows_host()?;
    let result = match operation {
        Operation::EnableWslFeatures => enable_wsl_features(operation),
    };
    match result {
        Ok(outcome) => {
            channel.send_result(&request.ticket_id, outcome)?;
            Ok(outcome)
        }
        Err(error) => {
            channel.send_result(&request.ticket_id, HelperOutcome::Failed)?;
            Err(error)
        }
    }
}

fn verify_release_identity() -> anyhow::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let expected = super::configured_publisher()?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let metadata = std::fs::symlink_metadata(&executable)?;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "helper image is not a regular file"
    );
    anyhow::ensure!(
        crate::execution::docker::engine::endpoint::named_pipe::image::signature::
            authenticode_publisher(&executable)
            .as_deref()
            == Some(expected),
        "Windows setup helper publisher trust is unavailable"
    );
    Ok(())
}

fn is_elevated() -> anyhow::Result<bool> {
    let mut token = std::ptr::null_mut();
    anyhow::ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "could not inspect Windows elevation state"
    );
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    unsafe { CloseHandle(token) };
    anyhow::ensure!(
        queried != 0 && returned == std::mem::size_of::<TOKEN_ELEVATION>() as u32,
        "could not verify Windows elevation state"
    );
    Ok(elevation.TokenIsElevated != 0)
}

fn enable_wsl_features(operation: Operation) -> anyhow::Result<HelperOutcome> {
    host::validate_supported_windows_host()?;
    let dism = system_tool("dism.exe")?;
    let publisher = crate::execution::docker::engine::endpoint::named_pipe::image::signature::
        authenticode_publisher(&dism);
    anyhow::ensure!(
        matches!(
            publisher.as_deref(),
            Some("Microsoft Windows" | "Microsoft Corporation")
        ),
        "Windows Deployment Image Servicing tool is not signed by Microsoft"
    );
    for feature in operation.features() {
        if run_dism(&dism, feature)? == HelperOutcome::RebootRequired {
            // A successful DISM operation that requires a restart ends this
            // helper pass. The next run must follow a fresh readiness scan.
            return Ok(HelperOutcome::RebootRequired);
        }
    }
    Ok(HelperOutcome::Completed)
}

fn run_dism(dism: &std::path::Path, feature: &str) -> anyhow::Result<HelperOutcome> {
    anyhow::ensure!(
        matches!(
            feature,
            "Microsoft-Windows-Subsystem-Linux" | "VirtualMachinePlatform"
        ),
        "unsupported Windows feature"
    );
    let mut child = Command::new(dism)
        .args([
            "/Online",
            "/Enable-Feature",
            &format!("/FeatureName:{feature}"),
            "/All",
            "/NoRestart",
            "/English",
        ])
        .env_clear()
        .env(
            "SYSTEMROOT",
            dism.parent()
                .and_then(std::path::Path::parent)
                .unwrap_or(dism.parent().unwrap()),
        )
        .current_dir(dism.parent().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED)
        .spawn()?;
    let _tree = match crate::harness::discovery::windows::WindowsProcessTree::attach(child.id()) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };
    let deadline = Instant::now() + MAX_OPERATION_TIME;
    loop {
        if let Some(status) = child.try_wait()? {
            return dism_outcome(status.code().unwrap_or(-1))
                .ok_or_else(|| anyhow::anyhow!("Windows feature enablement failed"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Windows feature enablement exceeded its time limit");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn system_tool(name: &str) -> anyhow::Result<PathBuf> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    anyhow::ensure!(name == "dism.exe", "unsupported Microsoft system tool");
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    anyhow::ensure!(
        length > 0 && length < buffer.len(),
        "Windows system directory is unavailable"
    );
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])).join(name);
    let metadata = std::fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "Microsoft tool path is not a regular file"
    );
    Ok(path)
}
