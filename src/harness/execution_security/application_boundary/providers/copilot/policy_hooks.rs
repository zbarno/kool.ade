#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegOpenKeyExW,
    },
};

#[cfg(windows)]
const REGISTRY_POLICY_ROOT: &str = "SOFTWARE\\Policies\\GitHub\\Copilot";
#[cfg(windows)]
const POLICY_VALUE: &str = "Policy";

#[cfg(unix)]
pub(super) const SYSTEM_POLICY_HOOKS: &str = "/etc/github-copilot/policy.d";
#[cfg(windows)]
pub(super) const SYSTEM_POLICY_HOOKS: &str = r"C:\ProgramData\GitHub\Copilot\policy.d";

pub(super) fn ensure_no_registry_policy_hooks() -> anyhow::Result<()> {
    #[cfg(windows)]
    for policy in read_registry_policies()? {
        anyhow::ensure!(
            !policy_has_hooks(&policy)?,
            "GitHub Copilot has machine-wide registry hooks that execute on the host outside Kool.ad/e's Docker execution boundary"
        );
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn policy_has_hooks(policy: &str) -> anyhow::Result<bool> {
    let value: serde_json::Value = serde_json::from_str(policy)
        .map_err(|_| anyhow::anyhow!("GitHub Copilot machine policy is not valid JSON"))?;
    let Some(hooks) = value.get("hooks") else {
        return Ok(false);
    };
    let Some(events) = hooks.as_object() else {
        return Ok(true);
    };
    Ok(events
        .values()
        .any(|entries| entries.as_array().is_none_or(|array| !array.is_empty())))
}

#[cfg(windows)]
fn read_registry_policies() -> anyhow::Result<Vec<String>> {
    use std::ptr;

    let root_name = wide(REGISTRY_POLICY_ROOT);
    let mut root = ptr::null_mut();
    // SAFETY: `root_name` is NUL terminated and `root` is a valid output slot.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            root_name.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut root,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        status == ERROR_SUCCESS,
        "could not inspect Copilot machine policy registry"
    );
    let root = RegistryKey(root);
    read_registry_policies_at(root.0)
}

#[cfg(windows)]
fn read_registry_policies_at(root: HKEY) -> anyhow::Result<Vec<String>> {
    use std::ptr;

    let mut policies = Vec::new();
    if let Some(policy) = read_policy_value(root)? {
        policies.push(policy);
    }

    let mut index = 0;
    loop {
        let mut name = vec![0_u16; 512];
        let mut length = name.len() as u32;
        // SAFETY: `name` is writable for `length` UTF-16 units; optional output
        // pointers are null as permitted by RegEnumKeyExW.
        let status = unsafe {
            RegEnumKeyExW(
                root,
                index,
                name.as_mut_ptr(),
                &mut length,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        anyhow::ensure!(
            status == ERROR_SUCCESS,
            "could not enumerate Copilot machine policy registry"
        );
        anyhow::ensure!(
            length < name.len() as u32,
            "Copilot machine policy registry entry is too long"
        );
        let mut subkey = name[..length as usize].to_vec();
        subkey.push(0);
        let mut child = ptr::null_mut();
        // SAFETY: `subkey` is NUL terminated and contains a name returned by
        // RegEnumKeyExW.
        let status = unsafe {
            RegOpenKeyExW(
                root,
                subkey.as_ptr(),
                0,
                KEY_READ | KEY_WOW64_64KEY,
                &mut child,
            )
        };
        anyhow::ensure!(
            status == ERROR_SUCCESS,
            "could not open Copilot machine policy registry entry"
        );
        let child = RegistryKey(child);
        if let Some(policy) = read_policy_value(child.0)? {
            policies.push(policy);
        }
        index += 1;
    }
    Ok(policies)
}

#[cfg(windows)]
fn read_policy_value(key: HKEY) -> anyhow::Result<Option<String>> {
    use std::ptr;
    use windows_sys::Win32::System::Registry::RegQueryValueExW;

    let name = wide(POLICY_VALUE);
    let mut kind = 0;
    let mut bytes = 0_u32;
    // SAFETY: The value name is NUL terminated; null data queries the size.
    let status = unsafe {
        RegQueryValueExW(
            key,
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
            && bytes >= 2
            && bytes <= 65_536
            && bytes % 2 == 0,
        "Copilot machine policy registry value is unreadable or invalid"
    );
    let mut data = vec![0_u8; bytes as usize];
    // SAFETY: `data` has the size returned by the preceding registry query.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null_mut(),
            &mut kind,
            data.as_mut_ptr(),
            &mut bytes,
        )
    };
    anyhow::ensure!(
        status == ERROR_SUCCESS && kind == REG_SZ,
        "Copilot machine policy registry value changed while reading"
    );
    let units = data[..bytes as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        units.last() == Some(&0) && !units[..units.len() - 1].contains(&0),
        "Copilot machine policy registry value has invalid string termination"
    );
    String::from_utf16(&units[..units.len() - 1])
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Copilot machine policy registry value is not valid UTF-16"))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
struct RegistryKey(HKEY);

#[cfg(windows)]
impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` is owned by this wrapper and closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
#[path = "policy_hooks/tests.rs"]
mod tests;
