use super::policy_has_hooks;

#[test]
fn empty_or_unrelated_machine_policies_do_not_block_copilot() {
    for policy in [r#"{"version":1}"#, r#"{"version":1,"hooks":{}}"#] {
        assert!(!policy_has_hooks(policy).unwrap());
    }
}

#[test]
fn machine_hook_entries_block_copilot_even_when_malformed() {
    assert!(!policy_has_hooks(r#"{"hooks":{"sessionStart":[]}}"#).unwrap());
    assert!(policy_has_hooks(r#"{"hooks":{"sessionStart":[{"type":"command"}]}}"#).unwrap());
    assert!(policy_has_hooks(r#"{"hooks":true}"#).unwrap());
    assert!(policy_has_hooks("not-json").is_err());
}

/// Production boundary: asserts Copilot's platform-specific machine hook
/// directory selection. Filesystem rejection has separate unit coverage.
#[cfg(windows)]
#[test]
fn windows_machine_policy_hook_path_matches_copilot_location() {
    assert_eq!(
        std::path::Path::new(super::SYSTEM_POLICY_HOOKS),
        std::path::Path::new(r"C:\ProgramData\GitHub\Copilot\policy.d")
    );
}

/// Production boundary: exercises the same Windows registry enumeration and
/// value reader as the machine policy check using a unique synthetic key under
/// HKCU. Parser tests cannot cover registry decoding. It needs a Windows runner
/// and no external service; setup cost is low.
#[cfg(windows)]
#[test]
fn registry_enumerator_reads_hook_policy_from_child_key() {
    use super::{POLICY_VALUE, RegistryKey, read_registry_policies_at, wide};
    use std::ptr;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCreateKeyExW, RegSetValueExW,
    };

    let root_path = wide(&format!(
        "Software\\KooladeTests\\PolicyHooks-{}",
        std::process::id()
    ));
    let cleanup = RegistryTestKey(root_path.clone());
    let mut root = ptr::null_mut();
    // SAFETY: all strings and output pointers are valid for the API call.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            root_path.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            ptr::null(),
            &mut root,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let root = RegistryKey(root);
    let child_path = wide("Provider");
    let mut child = ptr::null_mut();
    // SAFETY: `child_path` and output pointer remain valid for the call.
    let status = unsafe {
        RegCreateKeyExW(
            root.0,
            child_path.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_WOW64_64KEY,
            ptr::null(),
            &mut child,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let child = RegistryKey(child);
    let value_name = wide(POLICY_VALUE);
    let value = wide(r#"{"hooks":{"sessionStart":[{"type":"command"}]}}"#);
    // SAFETY: `value` is UTF-16 data; the byte count matches its length.
    let status = unsafe {
        RegSetValueExW(
            child.0,
            value_name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * size_of::<u16>()) as u32,
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    drop(child);

    let policies = read_registry_policies_at(root.0).unwrap();
    assert_eq!(policies.len(), 1);
    assert!(policy_has_hooks(&policies[0]).unwrap());

    drop(root);
    cleanup.remove();
}

#[cfg(windows)]
struct RegistryTestKey(Vec<u16>);

#[cfg(windows)]
impl RegistryTestKey {
    fn remove(self) {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        // SAFETY: this removes only the unique synthetic key created above.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()) };
        assert_eq!(status, windows_sys::Win32::Foundation::ERROR_SUCCESS);
    }
}

#[cfg(windows)]
impl Drop for RegistryTestKey {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        // SAFETY: this removes only the unique synthetic key created above.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()) };
    }
}

/// Production boundary: checks that the documented machine registry path is
/// readable and fails closed when it contains hooks. No external dependency.
#[cfg(windows)]
#[test]
fn registry_policy_probe_only_blocks_for_hook_policy() {
    if let Err(error) = super::ensure_no_registry_policy_hooks() {
        assert!(
            error.to_string().contains("machine-wide registry hooks"),
            "registry inspection should fail closed without exposing policy contents: {error}"
        );
    }
}
