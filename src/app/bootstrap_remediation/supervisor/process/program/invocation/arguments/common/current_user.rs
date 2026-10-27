#[cfg(target_os = "linux")]
pub(super) fn validate_name(uid: u32, expected_name: &str) -> anyhow::Result<()> {
    use std::{ffi::CStr, ptr};

    anyhow::ensure!(
        uid != 0 && valid_login_name(expected_name),
        "rootless subordinate IDs require a valid non-root account"
    );
    let mut record = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut result = ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    anyhow::ensure!(
        status == 0 && !result.is_null(),
        "current remediation account no longer exists"
    );
    let record = unsafe { record.assume_init() };
    anyhow::ensure!(
        !record.pw_name.is_null()
            && unsafe { CStr::from_ptr(record.pw_name) }.to_str()? == expected_name,
        "current remediation account identity changed"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn validate_name(_uid: u32, _expected_name: &str) -> anyhow::Result<()> {
    anyhow::bail!("subordinate ID operations are Linux-only")
}

fn valid_login_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$'))
}
