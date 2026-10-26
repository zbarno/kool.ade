use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

pub(crate) const RELEASE: &str = "2.1.310";
pub(crate) const MANIFEST_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.310/manifest.json";
pub(crate) const SIGNATURE_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.310/manifest.json.sig";
pub(crate) const KEY_URL: &str = "https://downloads.claude.ai/keys/claude-code.asc";
pub(crate) const BINARY_URL: &str =
    "https://downloads.claude.ai/claude-code-releases/2.1.310/linux-x64/claude";
pub(crate) const KEY_FINGERPRINT: &str = "31DDDE24DDFAB679F42D7BD2BAA929FF1A7ECACE";
pub(crate) const BINARY_SHA256: &str =
    "b73676430af12f97316a798b8371f81d640f90a58bdcb8f3ce56a835bb47df56";
pub(crate) const BINARY_SIZE: u64 = 291_638_640;
#[cfg(target_os = "linux")]
const MAX_SYSTEM_FILE_BYTES: u64 = 16 * 1024;

pub(crate) fn supports_current_host() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::consts::ARCH == "x86_64"
            && os_release().is_ok_and(|contents| supported_os_release(&contents))
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

pub(crate) fn require_supported_host() -> anyhow::Result<()> {
    anyhow::ensure!(
        supports_current_host(),
        "automatic Claude Code installation is supported only on Ubuntu 24.04 x86_64"
    );
    Ok(())
}

pub(crate) fn current_target() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("the current user's home directory is unavailable"))?;
        anyhow::ensure!(
            home.is_absolute(),
            "the current user's home path is not absolute"
        );
        let canonical = home.canonicalize()?;
        anyhow::ensure!(
            canonical == home,
            "the current user's home path is redirected"
        );
        let metadata = std::fs::symlink_metadata(&home)?;
        anyhow::ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o022 == 0,
            "the current user's home directory is not safely owned"
        );
        Ok(home
            .join(".local/bin/claude")
            .to_string_lossy()
            .into_owned())
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Claude Code automatic installation is unavailable on this platform")
    }
}

pub(crate) fn validate_target(target: &str) -> anyhow::Result<PathBuf> {
    let expected = current_target()?;
    anyhow::ensure!(target == expected, "Claude Code install target changed");
    Ok(PathBuf::from(expected))
}

#[cfg(target_os = "linux")]
fn os_release() -> anyhow::Result<String> {
    let file = File::open("/etc/os-release")?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= MAX_SYSTEM_FILE_BYTES,
        "Linux release metadata is outside policy"
    );
    anyhow::ensure!(
        metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "Linux release metadata is not root-owned and immutable to this user"
    );
    let mut contents = String::new();
    file.take(MAX_SYSTEM_FILE_BYTES + 1)
        .read_to_string(&mut contents)?;
    anyhow::ensure!(
        contents.len() as u64 <= MAX_SYSTEM_FILE_BYTES,
        "Linux release metadata exceeded its size limit"
    );
    Ok(contents)
}

fn supported_os_release(contents: &str) -> bool {
    os_release_value(contents, "ID") == Some("ubuntu")
        && os_release_value(contents, "VERSION_ID") == Some("24.04")
}

fn os_release_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::trim)
        .map(|value| value.trim_matches(['"', '\'']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_gate_requires_the_exact_distribution_release_and_architecture() {
        assert!(supported_os_release("ID=ubuntu\nVERSION_ID=24.04\n"));
        assert!(supported_os_release(
            "NAME=Ubuntu\nID=\"ubuntu\"\nVERSION_ID=\"24.04\"\n"
        ));
        for release in [
            "ID=ubuntu\nVERSION_ID=24.10\n",
            "ID=pop\nVERSION_ID=24.04\n",
            "ID=debian\nVERSION_ID=12\n",
            "ID=ubuntu\nVERSION_ID=24.04.1\n",
        ] {
            assert!(!supported_os_release(release));
        }
    }

    #[test]
    fn target_cannot_be_changed_to_a_model_supplied_path() {
        let expected = current_target().unwrap();
        assert!(validate_target(&expected).is_ok());
        assert!(validate_target("/tmp/claude").is_err());
        assert!(validate_target("/home/other/.local/bin/claude").is_err());
    }
}
