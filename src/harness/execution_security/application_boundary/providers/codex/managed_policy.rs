#[cfg(windows)]
use super::super::managed_hooks;
#[cfg(any(windows, test))]
use std::path::PathBuf;

#[cfg(windows)]
pub(super) fn ensure_no_system_host_execution() -> anyhow::Result<()> {
    let program_data = program_data_directory()?;
    let system = system_directory(&program_data);
    let settings = ["config.toml", "managed_config.toml", "requirements.toml"]
        .into_iter()
        .map(|name| system.join(name))
        .collect::<Vec<_>>();
    managed_hooks::ensure_no_toml_host_execution("Codex", settings.iter().map(PathBuf::as_path))?;
    managed_hooks::ensure_no_hook_file(&system.join("hooks.json"), "Codex")?;
    if let Some(requirements) = configured_requirements_path()? {
        managed_hooks::ensure_no_toml_host_execution("Codex", [requirements.as_path()])?;
    }
    Ok(())
}

#[cfg(windows)]
fn program_data_directory() -> anyhow::Result<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };

    let mut path = ptr::null_mut();
    // SAFETY: the known-folder identifier and output pointer are valid. The
    // returned allocation is released with the documented CoTaskMemFree API.
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, ptr::null_mut(), &mut path) };
    anyhow::ensure!(
        result >= 0 && !path.is_null(),
        "Codex managed system policy location is unavailable"
    );
    let result = unsafe {
        let mut length = 0;
        while *path.add(length) != 0 {
            length += 1;
        }
        PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
            path, length,
        )))
    };
    // SAFETY: `path` is the allocation returned by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(path.cast()) };
    Ok(result)
}

#[cfg(windows)]
fn configured_requirements_path() -> anyhow::Result<Option<PathBuf>> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        },
    };
    const POLICY_KEY: &str = "SOFTWARE\\Policies\\OpenAI\\Codex";
    const VALUE: &str = "RequirementsPath";
    let policy_key = wide(POLICY_KEY);
    let mut handle = ptr::null_mut();
    // SAFETY: the path is NUL terminated and handle is a valid output slot.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            policy_key.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut handle,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "could not inspect Codex Windows managed policy"
    );
    struct Key(windows_sys::Win32::System::Registry::HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }
    let handle = Key(handle);
    let name = wide(VALUE);
    let mut kind = 0;
    let mut bytes = 0;
    // SAFETY: null data requests only the value type and size.
    let status = unsafe {
        RegQueryValueExW(
            handle.0,
            name.as_ptr(),
            ptr::null_mut(),
            &mut kind,
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    anyhow::ensure!(
        status == ERROR_SUCCESS
            && kind == REG_SZ
            && (2..=32_768).contains(&bytes)
            && bytes.is_multiple_of(2),
        "Codex managed requirements path is unreadable or invalid"
    );
    let mut data = vec![0u16; (bytes / 2) as usize];
    // SAFETY: `data` is allocated to the exact size reported by the registry.
    let status = unsafe {
        RegQueryValueExW(
            handle.0,
            name.as_ptr(),
            ptr::null_mut(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "could not read Codex managed requirements path"
    );
    anyhow::ensure!(
        data.last() == Some(&0),
        "Codex managed requirements path is not NUL terminated"
    );
    let value = String::from_utf16(&data[..data.len().saturating_sub(1)])
        .map_err(|_| anyhow::anyhow!("Codex managed requirements path is invalid UTF-16"))?;
    let path = PathBuf::from(value);
    anyhow::ensure!(
        path.is_absolute(),
        "Codex managed requirements path must be absolute"
    );
    Ok(Some(path))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(any(windows, test))]
fn system_directory(program_data: &std::path::Path) -> PathBuf {
    program_data.join("OpenAI").join("Codex")
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_no_forced_host_execution_preferences() -> anyhow::Result<()> {
    use core_foundation::{base::TCFType, string::CFString};
    use core_foundation_sys::preferences::CFPreferencesAppValueIsForced;

    let domain = CFString::new("com.openai.codex");
    for key in [
        "hooks",
        "notify",
        "mcp_servers",
        "plugins",
        "features",
        "allow_managed_hooks_only",
    ] {
        let key = CFString::new(key);
        // SAFETY: both CFString values remain alive for the duration of the
        // call and provide valid application/key references.
        let forced = unsafe {
            CFPreferencesAppValueIsForced(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
        };
        anyhow::ensure!(
            forced == 0,
            "Codex has administrator-managed `{}` preferences that can execute host commands outside Kool.ad/e's Docker execution boundary",
            key
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::system_directory;
    use std::{fs, path::Path};

    #[test]
    fn windows_managed_system_config_paths_are_under_program_data() {
        let path = system_directory(Path::new("synthetic-program-data"));
        assert_eq!(path, Path::new("synthetic-program-data/OpenAI/Codex"));
    }

    #[test]
    fn windows_managed_requirements_with_hooks_are_rejected() {
        let root = std::env::temp_dir().join(format!(
            "koolade-codex-system-policy-{}",
            uuid::Uuid::new_v4()
        ));
        let system = system_directory(&root);
        fs::create_dir_all(&system).unwrap();
        let requirements = system.join("requirements.toml");
        fs::write(&requirements, "[hooks]\n").unwrap();
        let result = super::super::super::managed_hooks::ensure_no_toml_host_execution(
            "Codex",
            [requirements.as_path()],
        );
        assert!(result.is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
