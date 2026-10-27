use super::super::validate_os_version;

pub(in crate::app::bootstrap_remediation::identity) fn linux_os_version() -> anyhow::Result<String>
{
    let release = std::fs::read_to_string("/etc/os-release")?;
    let (distribution, version) = parse_os_release(&release)?;
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        value => value,
    };
    let kernel = kernel_release()?;
    let value = format!("{distribution}-{version}-{architecture}-kernel-{kernel}");
    validate_os_version(&value)?;
    Ok(value)
}

fn parse_os_release(contents: &str) -> anyhow::Result<(String, String)> {
    let distribution = field(contents, "ID")
        .ok_or_else(|| anyhow::anyhow!("Linux distribution ID is unavailable"))?;
    let version = field(contents, "VERSION_ID")
        .ok_or_else(|| anyhow::anyhow!("Linux distribution version is unavailable"))?;
    anyhow::ensure!(
        !distribution.is_empty()
            && distribution.len() <= 32
            && distribution
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
        "Linux distribution ID has an invalid format"
    );
    anyhow::ensure!(
        !version.is_empty()
            && version.len() <= 24
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte)),
        "Linux distribution version has an invalid format"
    );
    Ok((distribution.to_ascii_lowercase(), version.to_owned()))
}

fn field<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    let mut matches = contents
        .lines()
        .filter_map(|line| line.strip_prefix(key)?.strip_prefix('=').map(str::trim));
    let value = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let unquoted = match value.as_bytes().first() {
        Some(b'"') if value.ends_with('"') => value.get(1..value.len().checked_sub(1)?)?,
        Some(b'\'') if value.ends_with('\'') => value.get(1..value.len().checked_sub(1)?)?,
        Some(b'"' | b'\'') => return None,
        _ => value,
    };
    (!unquoted.is_empty()
        && unquoted
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte)))
    .then_some(unquoted)
}

fn kernel_release() -> anyhow::Result<String> {
    let mut info = std::mem::MaybeUninit::<libc::utsname>::zeroed();
    anyhow::ensure!(
        unsafe { libc::uname(info.as_mut_ptr()) } == 0,
        "could not read Linux kernel version"
    );
    let info = unsafe { info.assume_init() };
    let value = unsafe { std::ffi::CStr::from_ptr(info.release.as_ptr()) }
        .to_str()?
        .to_owned();
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte)),
        "Linux kernel version has an invalid format"
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_identity_version_binds_exact_distro_release_architecture_and_kernel() {
        let (distribution, version) =
            parse_os_release("ID=Ubuntu\nVERSION_ID=\"24.04\"\n").unwrap();
        assert_eq!(distribution, "ubuntu");
        assert_eq!(version, "24.04");
        assert_eq!(
            format!("{distribution}-{version}-amd64-kernel-6.8.0-31-generic"),
            "ubuntu-24.04-amd64-kernel-6.8.0-31-generic"
        );
    }

    #[test]
    fn linux_identity_rejects_missing_duplicate_and_malformed_release_fields() {
        for contents in [
            "VERSION_ID=24.04\n",
            "ID=ubuntu\nID=debian\nVERSION_ID=24.04\n",
            "ID=ubuntu\nVERSION_ID=24.04\nVERSION_ID=22.04\n",
            "ID=ubuntu\nVERSION_ID=\"24.04\n",
            "ID=ubuntu\nVERSION_ID=24.04;touch\n",
        ] {
            assert!(parse_os_release(contents).is_err(), "{contents:?}");
        }
    }
}
