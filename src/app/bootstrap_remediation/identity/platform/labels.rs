#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::app::bootstrap_remediation::identity) fn unix_machine_label() -> anyhow::Result<String>
{
    let mut name = [0_i8; 256];
    anyhow::ensure!(
        unsafe { libc::gethostname(name.as_mut_ptr(), name.len()) } == 0,
        "could not read the local machine name"
    );
    let end = name
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| anyhow::anyhow!("local machine name is oversized"))?;
    let value = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_string_lossy();
    let label = super::super::sanitize_display_label(&value);
    anyhow::ensure!(end > 0 && !label.is_empty(), "local machine name is empty");
    Ok(label)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::app::bootstrap_remediation::identity) fn unix_principal_label()
-> anyhow::Result<String> {
    use std::{ffi::CStr, mem::MaybeUninit, ptr};

    let uid = unsafe { libc::geteuid() };
    let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let buffer_size = if suggested <= 0 {
        16 * 1024
    } else {
        (suggested as usize).clamp(1024, 64 * 1024)
    };
    let mut buffer = vec![0_u8; buffer_size];
    let mut record = MaybeUninit::<libc::passwd>::zeroed();
    let mut result = ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    let label = if status == 0 && !result.is_null() {
        let record = unsafe { record.assume_init() };
        unsafe { CStr::from_ptr(record.pw_name) }
            .to_string_lossy()
            .into_owned()
    } else {
        format!("UID {uid}")
    };
    Ok(super::super::sanitize_display_label(&label))
}

#[cfg(target_os = "windows")]
pub(in crate::app::bootstrap_remediation::identity) fn windows_machine_label()
-> anyhow::Result<String> {
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNamePhysicalDnsHostname, GetComputerNameExW,
    };

    let mut buffer = [0_u16; 256];
    let mut length = buffer.len() as u32;
    anyhow::ensure!(
        unsafe {
            GetComputerNameExW(
                ComputerNamePhysicalDnsHostname,
                buffer.as_mut_ptr(),
                &mut length,
            )
        } != 0
            && length > 0
            && length < buffer.len() as u32,
        "could not read the local machine name"
    );
    let value = String::from_utf16(&buffer[..length as usize])?;
    let label = super::super::sanitize_display_label(&value);
    anyhow::ensure!(!label.is_empty(), "local machine name is empty");
    Ok(label)
}

#[cfg(target_os = "windows")]
pub(in crate::app::bootstrap_remediation::identity) fn windows_principal_label()
-> anyhow::Result<String> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{
            GetLengthSid, GetTokenInformation, LookupAccountSidW, SID_NAME_USE, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    anyhow::ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "could not inspect the current account identity"
    );
    let mut needed = 0_u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    anyhow::ensure!(
        needed >= size_of::<TOKEN_USER>() as u32 && needed <= 4096,
        "Windows account identity has an invalid size"
    );
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; words];
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            (words * size_of::<usize>()) as u32,
            &mut needed,
        )
    };
    unsafe { CloseHandle(token) };
    anyhow::ensure!(read != 0, "could not read the current account identity");

    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
    anyhow::ensure!(
        (8..=256).contains(&sid_length),
        "Windows account identity has an invalid SID size"
    );
    let mut name_length = 0_u32;
    let mut domain_length = 0_u32;
    let mut name_use: SID_NAME_USE = 0;
    unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            user.User.Sid,
            std::ptr::null_mut(),
            &mut name_length,
            std::ptr::null_mut(),
            &mut domain_length,
            &mut name_use,
        )
    };
    anyhow::ensure!(
        name_length > 0 && name_length <= 256 && domain_length <= 256,
        "Windows account name is outside display limits"
    );
    let mut name = vec![0_u16; name_length as usize];
    let mut domain = vec![0_u16; domain_length as usize];
    let resolved = unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            user.User.Sid,
            name.as_mut_ptr(),
            &mut name_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut name_use,
        )
    };
    anyhow::ensure!(resolved != 0, "could not resolve the current account name");
    let name = String::from_utf16(&name)?.trim_matches('\0').to_owned();
    let domain = String::from_utf16(&domain)?.trim_matches('\0').to_owned();
    let label = if domain.is_empty() {
        name
    } else {
        format!("{domain}\\{name}")
    };
    Ok(super::super::sanitize_display_label(&label))
}
