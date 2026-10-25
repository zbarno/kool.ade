#[cfg(target_os = "windows")]
use sha2::{Digest, Sha256};

#[cfg(target_os = "windows")]
use super::validate_os_version;

mod labels;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use labels::{unix_machine_label, unix_principal_label};
#[cfg(target_os = "windows")]
pub(super) use labels::{windows_machine_label, windows_principal_label};

#[cfg(target_os = "linux")]
pub(super) fn linux_os_version() -> anyhow::Result<String> {
    let mut info = std::mem::MaybeUninit::<libc::utsname>::zeroed();
    anyhow::ensure!(
        unsafe { libc::uname(info.as_mut_ptr()) } == 0,
        "could not read Linux version"
    );
    let info = unsafe { info.assume_init() };
    let value = unsafe { std::ffi::CStr::from_ptr(info.release.as_ptr()) }.to_str()?;
    Ok(value.to_owned())
}

#[cfg(target_os = "macos")]
pub(super) fn macos_machine_id() -> anyhow::Result<String> {
    use std::{ffi::CString, ptr};

    let key = CString::new("kern.uuid")?;
    let mut bytes = [0_u8; 128];
    let mut length = bytes.len();
    let result = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    anyhow::ensure!(
        result == 0 && length > 0 && length <= bytes.len(),
        "could not read the macOS machine UUID"
    );
    let value = std::str::from_utf8(&bytes[..length])?.trim_end_matches('\0');
    anyhow::ensure!(
        value.len() >= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'),
        "macOS machine UUID has an invalid format"
    );
    Ok(value.to_ascii_lowercase())
}

#[cfg(target_os = "macos")]
pub(super) fn macos_os_version() -> anyhow::Result<String> {
    use std::{ffi::CString, ptr};

    let key = CString::new("kern.osproductversion")?;
    let mut bytes = [0_u8; 64];
    let mut length = bytes.len();
    let result = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    anyhow::ensure!(
        result == 0 && length > 0 && length <= bytes.len(),
        "could not read macOS product version"
    );
    Ok(std::str::from_utf8(&bytes[..length])?
        .trim_end_matches('\0')
        .to_owned())
}

#[cfg(target_os = "windows")]
pub(super) fn windows_machine_id() -> anyhow::Result<String> {
    use std::ptr;
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ, RegGetValueW,
    };

    let key: Vec<u16> = "SOFTWARE\\Microsoft\\Cryptography\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "MachineGuid\0".encode_utf16().collect();
    let mut value_type = 0;
    let mut byte_len = 0;
    let first = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            &mut value_type,
            ptr::null_mut(),
            &mut byte_len,
        )
    };
    anyhow::ensure!(
        first == 0 && value_type == REG_SZ && byte_len >= 4 && byte_len <= 512,
        "could not read a trusted Windows machine identity"
    );
    let mut buffer = vec![0_u16; byte_len as usize / 2];
    let second = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            &mut value_type,
            buffer.as_mut_ptr().cast(),
            &mut byte_len,
        )
    };
    anyhow::ensure!(
        second == 0 && value_type == REG_SZ,
        "Windows machine identity changed while it was read"
    );
    let value = String::from_utf16(&buffer)?;
    let value = value.trim_matches('\0').trim();
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 128,
        "Windows machine identity has an invalid format"
    );
    Ok(value.to_ascii_lowercase())
}

#[cfg(target_os = "windows")]
pub(super) fn windows_os_version() -> anyhow::Result<String> {
    use std::ptr;
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

    let key: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "DisplayVersion\0".encode_utf16().collect();
    let mut byte_len = 0;
    let first = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut byte_len,
        )
    };
    anyhow::ensure!(
        first == 0 && byte_len >= 2 && byte_len <= 128,
        "could not read Windows version"
    );
    let mut buffer = vec![0_u16; byte_len as usize / 2];
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut byte_len,
        )
    };
    anyhow::ensure!(read == 0, "Windows version changed while it was read");
    let version = String::from_utf16(&buffer)?.trim_matches('\0').to_owned();
    validate_os_version(&version)?;
    Ok(version)
}

#[cfg(target_os = "windows")]
pub(super) fn windows_principal_id() -> anyhow::Result<String> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    anyhow::ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "could not inspect the current Windows user identity"
    );
    let mut needed = 0_u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    anyhow::ensure!(
        needed >= size_of::<TOKEN_USER>() as u32 && needed <= 4096,
        "Windows user identity has an invalid size"
    );
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (words * size_of::<usize>()) as u32,
            &mut needed,
        )
    };
    unsafe { CloseHandle(token) };
    anyhow::ensure!(
        read != 0,
        "could not read the current Windows user identity"
    );
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let sid_len = unsafe { GetLengthSid(user.User.Sid) } as usize;
    anyhow::ensure!(
        sid_len >= 8 && sid_len <= 256,
        "Windows user SID has an invalid size"
    );
    let sid = unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_len) };
    Ok(format!("sid-sha256-{:x}", Sha256::digest(sid)))
}
