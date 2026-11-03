#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_EXPAND_SZ,
        REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    },
};

#[cfg(windows)]
const ADMIN_POLICY_KEY: &str = "SOFTWARE\\Policies\\ClaudeCode";
#[cfg(windows)]
const USER_POLICY_KEY: &str = "SOFTWARE\\Policies\\ClaudeCode";
#[cfg(windows)]
const SETTINGS_VALUE: &str = "Settings";

#[cfg(windows)]
pub(super) fn ensure_no_windows_registry_settings() -> anyhow::Result<()> {
    for (hive, path, source) in [
        (HKEY_LOCAL_MACHINE, ADMIN_POLICY_KEY, "machine"),
        (HKEY_CURRENT_USER, USER_POLICY_KEY, "user"),
    ] {
        anyhow::ensure!(
            !registry_has_settings(hive, path)?,
            "Claude Code has Windows {source} managed settings that may run host commands outside Kool.ad/e's Docker execution boundary"
        );
    }
    Ok(())
}

#[cfg(windows)]
fn registry_has_settings(hive: HKEY, path: &str) -> anyhow::Result<bool> {
    use std::ptr;

    let path = wide(path);
    let mut key = ptr::null_mut();
    // SAFETY: `path` is NUL terminated and `key` is a valid output slot.
    let status =
        unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut key) };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(false);
    }
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "could not inspect Claude Code Windows managed settings"
    );
    let key = RegistryKey(key);
    let value_name = wide(SETTINGS_VALUE);
    let mut value_type = 0;
    let mut byte_length = 0;
    // SAFETY: the value name is NUL terminated; a null data pointer requests
    // only the registry value's type and required size.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name.as_ptr(),
            ptr::null_mut(),
            &mut value_type,
            ptr::null_mut(),
            &mut byte_length,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    anyhow::ensure!(
        status == ERROR_SUCCESS
            && (value_type == REG_SZ || value_type == REG_EXPAND_SZ)
            && (2..=1_048_576).contains(&byte_length)
            && byte_length.is_multiple_of(2),
        "Claude Code Windows managed settings registry value is unreadable or invalid"
    );
    Ok(true)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
struct RegistryKey(HKEY);

#[cfg(windows)]
impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this wrapper and closed exactly once.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_no_forced_host_execution_preferences() -> anyhow::Result<()> {
    use core_foundation::{base::TCFType, string::CFString};
    use core_foundation_sys::preferences::CFPreferencesAppValueIsForced;

    let domain = CFString::new("com.anthropic.claudecode");
    for key in [
        "hooks",
        "apiKeyHelper",
        "awsCredentialExport",
        "managedMcpServers",
        "statusLine",
        "fileSuggestion",
        "subagentStatusLine",
        "policyHelper",
        "enabledPlugins",
        "allowManagedHooksOnly",
    ] {
        let key = CFString::new(key);
        // SAFETY: both CFString values remain alive for the duration of the
        // call and provide valid application/key references.
        let forced = unsafe {
            CFPreferencesAppValueIsForced(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
        };
        anyhow::ensure!(
            forced == 0,
            "Claude Code has administrator-managed `{}` preferences that may run host commands outside Kool.ad/e's Docker execution boundary",
            key
        );
    }
    Ok(())
}

#[cfg(windows)]
#[cfg(test)]
mod tests {
    use super::{RegistryKey, registry_has_settings, wide};
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_OPTION_NON_VOLATILE,
            REG_SZ, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
        },
    };

    /// Production boundary: exercises the same HKCU registry lookup used by
    /// the launch preflight. Parser tests cannot prove the Win32 value reader
    /// and hive selection. It uses a unique synthetic key on a Windows runner,
    /// with no external service and low setup cost.
    #[test]
    fn reads_settings_value_from_synthetic_user_policy_key() {
        let path = wide(&format!(
            "Software\\KooladeTests\\ClaudePolicy-{}",
            std::process::id()
        ));
        let cleanup = RegistryTestKey(path.clone());
        let mut key = ptr::null_mut();
        // SAFETY: the path and output pointer are valid for the registry call.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let key = RegistryKey(key);
        let value_name = wide("Settings");
        let value = wide(r#"{"hooks":{"SessionStart":[]}}"#);
        // SAFETY: `value` is UTF-16 data and its byte length is exact.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                value_name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * size_of::<u16>()) as u32,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        assert!(
            registry_has_settings(
                HKEY_CURRENT_USER,
                &String::from_utf16_lossy(&path[..path.len() - 1])
            )
            .unwrap()
        );
        drop(key);
        cleanup.remove();
    }

    struct RegistryTestKey(Vec<u16>);

    impl RegistryTestKey {
        fn remove(self) {
            // SAFETY: this removes only the unique synthetic key created above.
            let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()) };
            assert_eq!(status, ERROR_SUCCESS);
        }
    }

    impl Drop for RegistryTestKey {
        fn drop(&mut self) {
            // SAFETY: this removes only the unique synthetic test key.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()) };
        }
    }
}
