use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
        RegCloseKey, RegGetValueW, RegOpenKeyExW,
    },
};

use super::super::super::{ProbeFact, ProbeRead};
use super::super::classify::reboot_fact;

pub(super) fn registry_string(key: &str, name: &str) -> ProbeRead<String> {
    let key = wide(key);
    let name = wide(name);
    let mut value_type = 0;
    let mut byte_length = 0_u32;
    let first = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut value_type,
            std::ptr::null_mut(),
            &mut byte_length,
        )
    };
    if let Some(result) = registry_result(first) {
        return result;
    }
    if value_type != REG_SZ || !(2..=512).contains(&byte_length) || byte_length % 2 != 0 {
        return ProbeRead::Malformed;
    }
    let mut buffer = vec![0_u16; byte_length as usize / 2];
    let second = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut value_type,
            buffer.as_mut_ptr().cast(),
            &mut byte_length,
        )
    };
    if second != ERROR_SUCCESS || value_type != REG_SZ {
        return ProbeRead::Inaccessible;
    }
    match String::from_utf16(&buffer) {
        Ok(value) => {
            let value = value.trim_matches('\0').trim();
            if value.is_empty()
                || value.len() > 128
                || value.bytes().any(|byte| byte.is_ascii_control())
            {
                ProbeRead::Malformed
            } else {
                ProbeRead::Found(value.to_owned())
            }
        }
        Err(_) => ProbeRead::Malformed,
    }
}

pub(super) fn wsl_policy() -> ProbeRead<Option<bool>> {
    let key = wide("Software\\Policies\\WSL");
    let value = wide("AllowWSL");
    let mut value_type = 0;
    let mut allowed = 0_u32;
    let mut byte_length = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut value_type,
            (&mut allowed as *mut u32).cast(),
            &mut byte_length,
        )
    };
    match registry_result::<Option<bool>>(status) {
        None if value_type == REG_DWORD && byte_length == 4 && allowed <= 1 => {
            ProbeRead::Found(Some(allowed == 1))
        }
        None => ProbeRead::Malformed,
        Some(ProbeRead::Missing) => ProbeRead::Missing,
        Some(ProbeRead::Inaccessible) => ProbeRead::Inaccessible,
        Some(ProbeRead::Malformed) => ProbeRead::Malformed,
        Some(ProbeRead::Found(_)) => ProbeRead::Malformed,
    }
}

pub(super) fn pending_reboot() -> ProbeFact {
    let keys = [
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending",
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired",
    ];
    let mut inaccessible = false;
    for key in keys {
        match registry_key_exists(key) {
            ProbeRead::Found(true) => return reboot_fact(ProbeRead::Found(true)),
            ProbeRead::Found(false) => {}
            ProbeRead::Missing => {}
            ProbeRead::Inaccessible | ProbeRead::Malformed => inaccessible = true,
        }
    }
    if inaccessible {
        reboot_fact(ProbeRead::Inaccessible)
    } else {
        reboot_fact(ProbeRead::Found(false))
    }
}

fn registry_key_exists(path: &str) -> ProbeRead<bool> {
    let path = wide(path);
    let mut key = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status == ERROR_SUCCESS {
        unsafe { RegCloseKey(key) };
        ProbeRead::Found(true)
    } else if status == ERROR_FILE_NOT_FOUND {
        ProbeRead::Found(false)
    } else if status == ERROR_ACCESS_DENIED {
        ProbeRead::Inaccessible
    } else {
        ProbeRead::Inaccessible
    }
}

fn registry_result<T>(status: u32) -> Option<ProbeRead<T>> {
    if status == ERROR_SUCCESS {
        None
    } else if status == ERROR_FILE_NOT_FOUND {
        Some(ProbeRead::Missing)
    } else if status == ERROR_ACCESS_DENIED {
        Some(ProbeRead::Inaccessible)
    } else {
        Some(ProbeRead::Inaccessible)
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
