use windows_sys::Win32::{
    Foundation::ERROR_FILE_NOT_FOUND,
    System::{
        Registry::{
            HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, REG_DWORD, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
            RegCloseKey, RegGetValueW, RegOpenKeyExW,
        },
        SystemInformation::{GetNativeSystemInfo, PROCESSOR_ARCHITECTURE_AMD64, SYSTEM_INFO},
        Threading::IsProcessorFeaturePresent,
    },
};

const PF_VIRT_FIRMWARE_ENABLED: u32 = 21;

pub(super) fn validate_supported_windows_host() -> anyhow::Result<()> {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetNativeSystemInfo(&mut info) };
    anyhow::ensure!(
        unsafe { info.Anonymous.Anonymous.wProcessorArchitecture } == PROCESSOR_ARCHITECTURE_AMD64,
        "Windows setup mutation is supported only on Windows 11 x64"
    );
    anyhow::ensure!(
        unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) } != 0,
        "firmware virtualization is unavailable; enable it through your device administrator"
    );
    let build = registry_string(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "CurrentBuildNumber",
    )?;
    anyhow::ensure!(
        build.parse::<u32>().is_ok_and(|build| build >= 22_000),
        "Windows setup mutation is supported only on Windows 11 x64"
    );
    let edition = registry_string(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "EditionID",
    )?;
    anyhow::ensure!(
        matches!(
            edition.as_str(),
            "Core"
                | "CoreN"
                | "Professional"
                | "ProfessionalN"
                | "Enterprise"
                | "EnterpriseN"
                | "Education"
                | "EducationN"
        ),
        "this Windows edition is outside the supported setup action"
    );
    let policy = registry_dword("Software\\Policies\\WSL", "AllowWSL")?;
    anyhow::ensure!(
        policy.is_none_or(|allowed| allowed == 1),
        "Windows policy blocks WSL; contact your administrator"
    );
    anyhow::ensure!(
        !pending_reboot()?,
        "Windows has a pending restart; restart and recheck before changing features"
    );
    Ok(())
}

fn registry_string(key_path: &str, value_name: &str) -> anyhow::Result<String> {
    let key = wide(key_path);
    let name = wide(value_name);
    let mut kind = 0;
    let mut bytes = 0;
    let first = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    anyhow::ensure!(first == 0 && (2..=512).contains(&bytes) && bytes.is_multiple_of(2));
    let mut buffer = vec![0_u16; bytes as usize / 2];
    let second = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    anyhow::ensure!(
        second == 0,
        "Windows host identity changed while being read"
    );
    let value = String::from_utf16(&buffer)?
        .trim_matches('\0')
        .trim()
        .to_owned();
    anyhow::ensure!(!value.is_empty(), "Windows host identity is empty");
    Ok(value)
}

fn registry_dword(key_path: &str, value_name: &str) -> anyhow::Result<Option<u32>> {
    let key = wide(key_path);
    let name = wide(value_name);
    let mut kind = 0;
    let mut value = 0_u32;
    let mut bytes = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut kind,
            (&mut value as *mut u32).cast(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    anyhow::ensure!(status == 0 && kind == REG_DWORD && bytes == 4 && value <= 1);
    Ok(Some(value))
}

fn pending_reboot() -> anyhow::Result<bool> {
    for path in [
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending",
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired",
    ] {
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
        if status == 0 {
            unsafe { RegCloseKey(key) };
            return Ok(true);
        }
        anyhow::ensure!(
            status == ERROR_FILE_NOT_FOUND,
            "Windows restart status could not be verified"
        );
    }
    Ok(false)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
