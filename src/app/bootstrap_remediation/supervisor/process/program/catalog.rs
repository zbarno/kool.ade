use std::path::PathBuf;

#[cfg(windows)]
use std::ffi::OsString;

use crate::app::bootstrap_repair_policy::Host;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustedProgram {
    AptGet,
    Dnf,
    Zypper,
    MacInstaller,
    WindowsMsiExec,
    #[cfg(test)]
    TestHelper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustedFlag {
    AptInstall,
    AssumeYes,
    Quiet,
    MsiInstall,
    NoRestart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativePackage {
    DockerEngine,
    DockerCli,
    Containerd,
    ComposePlugin,
}

impl NativePackage {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::DockerEngine => "docker-ce",
            Self::DockerCli => "docker-ce-cli",
            Self::Containerd => "containerd.io",
            Self::ComposePlugin => "docker-compose-plugin",
        }
    }
}

impl TrustedProgram {
    pub(super) fn matches(self, host: Host) -> bool {
        match self {
            Self::AptGet | Self::Dnf | Self::Zypper => host == Host::Linux,
            Self::MacInstaller => host == Host::MacOs,
            Self::WindowsMsiExec => host == Host::Windows,
            #[cfg(test)]
            Self::TestHelper => true,
        }
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process) fn resolve(
        self,
        host: Host,
    ) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(
            self.matches(host),
            "trusted program is not approved for this platform"
        );
        match self {
            Self::AptGet => Ok(PathBuf::from("/usr/bin/apt-get")),
            Self::Dnf => Ok(PathBuf::from("/usr/bin/dnf")),
            Self::Zypper => Ok(PathBuf::from("/usr/bin/zypper")),
            Self::MacInstaller => Ok(PathBuf::from("/usr/sbin/installer")),
            Self::WindowsMsiExec => windows_msiexec_path(),
            #[cfg(test)]
            Self::TestHelper => Ok(std::env::current_exe()?),
        }
    }
}

#[cfg(windows)]
fn windows_msiexec_path() -> anyhow::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let mut buffer = [0_u16; 32768];
    let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    anyhow::ensure!(
        length > 0 && length < buffer.len(),
        "Windows system directory could not be resolved"
    );
    let mut path = PathBuf::from(OsString::from_wide(&buffer[..length]));
    path.push("msiexec.exe");
    Ok(path)
}

#[cfg(not(windows))]
fn windows_msiexec_path() -> anyhow::Result<PathBuf> {
    anyhow::bail!("Windows remediation programs are unavailable on this host")
}
