use std::path::PathBuf;

#[cfg(windows)]
use std::ffi::OsString;

use crate::app::bootstrap_repair_policy::Host;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustedProgram {
    AptGet,
    Usermod,
    Systemctl,
    Rootlesskit,
    True,
    SystemdRun,
    Pkexec,
    Dd,
    Mkdir,
    Find,
    Rm,
    Rpm,
    RpmDb,
    RpmKeys,
    Sha256sum,
    /// Internal, app-owned Linux per-user rootless Docker service operation.
    LinuxRootlessService,
    /// Internal, app-owned signed Docker repository and package bootstrap.
    LinuxRootlessPackageSetup,
    Dnf,
    Zypper,
    MacInstaller,
    WindowsMsiExec,
    ClaudeCodeInstall,
    ClaudeCodeDownload,
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
            Self::AptGet
            | Self::Usermod
            | Self::Systemctl
            | Self::Rootlesskit
            | Self::True
            | Self::SystemdRun
            | Self::Pkexec
            | Self::Dd
            | Self::Mkdir
            | Self::Find
            | Self::Rm
            | Self::Rpm
            | Self::RpmDb
            | Self::RpmKeys
            | Self::Sha256sum
            | Self::LinuxRootlessService
            | Self::LinuxRootlessPackageSetup
            | Self::Dnf
            | Self::Zypper => host == Host::Linux,
            Self::MacInstaller => host == Host::MacOs,
            Self::WindowsMsiExec => host == Host::Windows,
            Self::ClaudeCodeInstall | Self::ClaudeCodeDownload => host == Host::Linux,
            #[cfg(test)]
            Self::TestHelper => true,
        }
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process) fn is_internal(self) -> bool {
        matches!(
            self,
            Self::LinuxRootlessService | Self::LinuxRootlessPackageSetup
        )
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
            Self::Usermod => Ok(PathBuf::from("/usr/sbin/usermod")),
            Self::Systemctl => Ok(PathBuf::from("/usr/bin/systemctl")),
            Self::Rootlesskit => Ok(PathBuf::from("/usr/bin/rootlesskit")),
            Self::True => Ok(PathBuf::from("/usr/bin/true")),
            Self::SystemdRun => Ok(PathBuf::from("/usr/bin/systemd-run")),
            Self::Pkexec => Ok(PathBuf::from("/usr/bin/pkexec")),
            Self::Dd => Ok(PathBuf::from("/usr/bin/dd")),
            Self::Mkdir => Ok(PathBuf::from("/usr/bin/mkdir")),
            Self::Find => Ok(PathBuf::from("/usr/bin/find")),
            Self::Rm => Ok(PathBuf::from("/usr/bin/rm")),
            Self::Rpm => Ok(PathBuf::from("/usr/bin/rpm")),
            Self::RpmDb => Ok(PathBuf::from("/usr/bin/rpmdb")),
            Self::RpmKeys => Ok(PathBuf::from("/usr/bin/rpmkeys")),
            Self::Sha256sum => Ok(PathBuf::from("/usr/bin/sha256sum")),
            Self::LinuxRootlessService => {
                anyhow::bail!("internal remediation programs must use the trusted app dispatcher")
            }
            Self::LinuxRootlessPackageSetup => {
                anyhow::bail!("internal remediation programs must use the trusted app dispatcher")
            }
            Self::Dnf => Ok(PathBuf::from("/usr/bin/dnf5")),
            Self::Zypper => Ok(PathBuf::from("/usr/bin/zypper")),
            Self::MacInstaller => Ok(PathBuf::from("/usr/sbin/installer")),
            Self::WindowsMsiExec => windows_msiexec_path(),
            Self::ClaudeCodeInstall => {
                anyhow::bail!("Claude Code installation is a typed internal broker operation")
            }
            Self::ClaudeCodeDownload => Ok(PathBuf::from("/usr/bin/curl")),
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
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_rootless_service_is_internal_and_has_no_external_path() {
        assert!(TrustedProgram::LinuxRootlessService.is_internal());
        assert!(
            TrustedProgram::LinuxRootlessService
                .resolve(Host::Linux)
                .is_err()
        );
        assert!(!TrustedProgram::Systemctl.is_internal());
    }

    #[test]
    fn rootless_external_programs_use_only_fixed_system_paths() {
        for (program, expected) in [
            (TrustedProgram::AptGet, "/usr/bin/apt-get"),
            (TrustedProgram::Usermod, "/usr/sbin/usermod"),
            (TrustedProgram::Systemctl, "/usr/bin/systemctl"),
            (TrustedProgram::Rootlesskit, "/usr/bin/rootlesskit"),
            (TrustedProgram::True, "/usr/bin/true"),
            (TrustedProgram::SystemdRun, "/usr/bin/systemd-run"),
            (TrustedProgram::Pkexec, "/usr/bin/pkexec"),
            (TrustedProgram::Dd, "/usr/bin/dd"),
            (TrustedProgram::Mkdir, "/usr/bin/mkdir"),
            (TrustedProgram::Find, "/usr/bin/find"),
            (TrustedProgram::Rm, "/usr/bin/rm"),
            (TrustedProgram::Rpm, "/usr/bin/rpm"),
            (TrustedProgram::RpmDb, "/usr/bin/rpmdb"),
            (TrustedProgram::RpmKeys, "/usr/bin/rpmkeys"),
            (TrustedProgram::Sha256sum, "/usr/bin/sha256sum"),
        ] {
            assert_eq!(
                program.resolve(Host::Linux).unwrap(),
                PathBuf::from(expected)
            );
        }
    }
}
