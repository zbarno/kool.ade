use std::fs::OpenOptions;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
    WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
    WinVerifyTrust,
};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

pub(super) fn authenticode_publisher(image_path: &Path) -> Option<String> {
    // Keep the opened file object stable during verification. Denying write/delete sharing also
    // prevents the path from being replaced while WinVerifyTrust hashes this handle.
    let image_file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(image_path)
        .ok()?;
    if !image_file.metadata().ok()?.is_file() {
        return None;
    }

    let path: Vec<u16> = image_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut trust_file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path.as_ptr(),
        hFile: std::os::windows::io::AsRawHandle::as_raw_handle(&image_file) as HANDLE,
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut trust = WINTRUST_DATA::default();
    trust.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
    trust.dwUIChoice = WTD_UI_NONE;
    trust.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    trust.dwUnionChoice = WTD_CHOICE_FILE;
    trust.Anonymous.pFile = &mut trust_file;
    trust.dwStateAction = WTD_STATEACTION_VERIFY;
    // Trust checks use the local certificate/revocation cache only. If required data is missing,
    // verification fails closed instead of contacting a network revocation endpoint.
    trust.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };

    let publisher = if status == ERROR_SUCCESS as i32 {
        signer_publisher(trust.hWVTStateData)
    } else {
        None
    };
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        );
    }
    publisher
}

fn signer_publisher(state: HANDLE) -> Option<String> {
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return None;
    }
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() || unsafe { (*signer).dwError } != 0 {
        return None;
    }
    let certificate = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return None;
    }
    let certificate_context = unsafe { (*certificate).pCert };
    if certificate_context.is_null() {
        return None;
    }

    let name_length = unsafe {
        CertGetNameStringW(
            certificate_context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        )
    } as usize;
    if name_length <= 1 {
        return None;
    }
    let mut name = vec![0_u16; name_length];
    let written = unsafe {
        CertGetNameStringW(
            certificate_context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            std::ptr::null(),
            name.as_mut_ptr(),
            name_length as u32,
        )
    } as usize;
    (written > 1).then(|| String::from_utf16_lossy(&name[..written - 1]))
}
