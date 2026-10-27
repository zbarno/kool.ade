use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{
    GetProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
};

const DOCKER_BACKEND_IMAGE: &str = "com.docker.backend.exe";
const DOCKER_PUBLISHER: &str = "Docker Inc.";

pub(crate) mod signature;

pub(super) struct VerifiedServerProcess {
    _process: ProcessHandle,
}

pub(super) struct ServerProcess(ProcessHandle);

impl VerifiedServerProcess {
    pub(super) fn ensure_running(&self) -> io::Result<()> {
        self._process.ensure_running()
    }
}

impl ServerProcess {
    pub(super) fn open(process_id: u32) -> io::Result<Self> {
        let process = ProcessHandle::open(process_id)?;
        process.ensure_running()?;
        if unsafe { GetProcessId(process.raw()) } != process_id {
            return Err(untrusted_image());
        }
        Ok(Self(process))
    }

    pub(super) fn validate(self) -> io::Result<VerifiedServerProcess> {
        let process = self.0;
        process.ensure_running()?;
        let image_path = process_image_path_for_current_user(&process)?;
        let canonical_image = std::fs::canonicalize(&image_path)?;
        let roots = docker_install_roots();
        if !is_in_expected_install_root(&canonical_image, &roots) {
            return Err(untrusted_image());
        }

        let publisher =
            signature::authenticode_publisher(&canonical_image).ok_or_else(untrusted_image)?;
        if !is_trusted_image_identity(&canonical_image.to_string_lossy(), &publisher) {
            return Err(untrusted_image());
        }
        process.ensure_running()?;
        Ok(VerifiedServerProcess { _process: process })
    }
}

fn process_image_path_for_current_user(process: &ProcessHandle) -> io::Result<PathBuf> {
    (|| {
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process.raw(), 0, buffer.as_mut_ptr(), &mut length) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let server_user = process_user_sid(process.raw())?;
        let current_user = super::acl::current_user_sid()?;
        if unsafe {
            EqualSid(
                server_user.as_ptr() as *mut _,
                current_user.as_ptr() as *mut _,
            )
        } == 0
        {
            return Err(untrusted_image());
        }
        Ok(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    })()
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> io::Result<Self> {
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(process))
    }

    fn raw(&self) -> HANDLE {
        self.0
    }

    fn ensure_running(&self) -> io::Result<()> {
        let status = unsafe { WaitForSingleObject(self.raw(), 0) };
        if status == WAIT_TIMEOUT {
            Ok(())
        } else if status == WAIT_OBJECT_0 {
            Err(untrusted_image())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn process_user_sid(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut required = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
        }
        if required < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err(untrusted_image());
        }
        let byte_len = required as usize;
        let word_len = byte_len.div_ceil(std::mem::size_of::<usize>());
        // TOKEN_USER contains pointers and must be read from suitably aligned storage.
        let mut buffer = vec![0_usize; word_len];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        if user.User.Sid.is_null() || unsafe { IsValidSid(user.User.Sid) } == 0 {
            return Err(untrusted_image());
        }
        let length = unsafe { windows_sys::Win32::Security::GetLengthSid(user.User.Sid) } as usize;
        if length == 0 {
            return Err(untrusted_image());
        }
        Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), length) }.to_vec())
    })();
    unsafe { CloseHandle(token) };
    result
}

fn docker_install_roots() -> Vec<PathBuf> {
    // These are Docker's documented default per-user and all-users install roots. Unknown custom
    // locations remain unsupported until verified against a real Desktop installation.
    let mut roots = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local_app_data).join("Programs/DockerDesktop/resources"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("Docker/Docker/resources"));
    }
    roots
}

fn is_in_expected_install_root(image_path: &Path, expected_roots: &[PathBuf]) -> bool {
    let Some(file_name) = image_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !file_name.eq_ignore_ascii_case(DOCKER_BACKEND_IMAGE) {
        return false;
    }
    let Some(parent) = image_path.parent() else {
        return false;
    };
    expected_roots
        .iter()
        .any(|root| same_windows_path(parent, root))
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        let value = path.to_string_lossy();
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(&value)
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }
    normalized(left) == normalized(right)
}

fn is_trusted_image_identity(image_path: &str, publisher: &str) -> bool {
    Path::new(image_path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(DOCKER_BACKEND_IMAGE))
        && publisher
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case(DOCKER_PUBLISHER.trim_end_matches('.'))
}

fn untrusted_image() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Docker named-pipe server image or signature is not trusted",
    )
}

#[cfg(test)]
mod tests {
    use super::{Path, PathBuf, is_in_expected_install_root, is_trusted_image_identity};

    #[test]
    fn accepts_only_the_expected_image_and_docker_publisher() {
        assert!(is_trusted_image_identity(
            r"C:\Program Files\Docker\Docker\resources\com.docker.backend.exe",
            "Docker Inc."
        ));
        assert!(!is_trusted_image_identity(
            r"C:\Program Files\Docker\Docker\resources\com.docker.backend.exe",
            "Docker Inc. Untrusted"
        ));
        assert!(!is_trusted_image_identity(
            r"C:\Program Files\Docker\Docker\resources\com.docker.proxy.exe",
            "Docker Inc."
        ));
    }

    #[test]
    fn rejects_backend_images_outside_supported_install_roots() {
        let roots = [PathBuf::from(r"C:\Program Files\Docker\Docker\resources")];
        assert!(is_in_expected_install_root(
            Path::new(r"C:\Program Files\Docker\Docker\resources\com.docker.backend.exe"),
            &roots
        ));
        assert!(!is_in_expected_install_root(
            Path::new(r"C:\Temp\com.docker.backend.exe"),
            &roots
        ));
    }
}
