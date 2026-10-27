use std::{ffi::CString, mem::size_of, ptr};

use crate::app::setup_attention::catalog::ImageArchitectureV1 as Architecture;

pub(super) fn product_version() -> Option<String> {
    let bytes = sysctl_bytes("kern.osproductversion")?;
    let version = std::str::from_utf8(&bytes).ok()?.trim_end_matches('\0');
    let mut parts = version.split('.');
    let major = parts.next()?;
    if major.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || parts.any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || version.len() > 32
    {
        return None;
    }
    Some(version.to_owned())
}

pub(super) fn physical_architecture() -> Architecture {
    let key = CString::new("hw.optional.arm64").expect("static sysctl name");
    let mut value = 0_u32;
    let mut length = size_of::<u32>();
    let result = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 || length != size_of::<u32>() {
        return Architecture::Unknown;
    }
    match value {
        0 => Architecture::Amd64,
        1 => Architecture::Arm64,
        _ => Architecture::Unknown,
    }
}

fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
    let key = CString::new(name).ok()?;
    let mut length = 0_usize;
    let first = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            ptr::null_mut(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    if first != 0 || !(1..=128).contains(&length) {
        return None;
    }
    let mut bytes = vec![0_u8; length];
    let read = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    if read != 0 || !(1..=bytes.len()).contains(&length) {
        return None;
    }
    bytes.truncate(length);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_physical_architecture_even_when_running_under_translation() {
        assert!(matches!(
            physical_architecture(),
            Architecture::Amd64 | Architecture::Arm64
        ));
    }

    #[test]
    fn reports_a_parseable_macos_product_version() {
        let version = product_version().expect("macOS product version");
        assert!(version.split('.').all(|part| part.parse::<u16>().is_ok()));
    }
}
