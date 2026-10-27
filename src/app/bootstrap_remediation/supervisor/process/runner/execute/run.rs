use super::super::super::{
    NativeCommandRunner, ProcessOutput, ProcessRunError, ProcessRunFailure, SystemCommandRunner,
    platform, program,
};
use crate::app::bootstrap_remediation::supervisor::{Cancellation, ElevationReceipt};
use program::TrustedInvocation;

use super::super::user_environment;
use super::{monitor, scope};

impl NativeCommandRunner for SystemCommandRunner {
    fn run(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        if invocation.program.is_internal() {
            #[cfg(target_os = "linux")]
            return match invocation.program() {
                program::TrustedProgram::LinuxRootlessService => {
                    super::super::rootless_service::execute(self, invocation, cancellation)
                }
                program::TrustedProgram::LinuxRootlessPackageSetup => {
                    Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable))
                }
                _ => Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable)),
            };
            #[cfg(not(target_os = "linux"))]
            return Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable));
        }
        self.execute(invocation, cancellation, false)
    }

    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        elevation: ElevationReceipt,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        if !matches!(elevation.matches_invocation(&invocation), Ok(true)) {
            return Err(ProcessRunError::not_started(
                ProcessRunFailure::FailedBeforeLaunch,
            ));
        }
        if crate::app::bootstrap_remediation::validate_before_elevated_launch(&invocation).is_err()
        {
            return Err(ProcessRunError::not_started(
                ProcessRunFailure::FailedBeforeLaunch,
            ));
        }
        #[cfg(target_os = "linux")]
        if invocation.program() == program::TrustedProgram::LinuxRootlessPackageSetup {
            return super::super::rootless_packages::execute(self, invocation, cancellation);
        }
        self.execute(invocation, cancellation, true)
    }
}

impl SystemCommandRunner {
    pub(in crate::app::bootstrap_remediation::supervisor) fn run_elevation_preflight(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.execute(invocation, cancellation, true)
    }

    pub(in crate::app::bootstrap_remediation::supervisor::process::runner) fn execute(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
        elevated: bool,
    ) -> Result<ProcessOutput, ProcessRunError> {
        use std::process::{Command, Stdio};

        if !platform::platform_execution_supported() {
            return Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable));
        }
        if cancellation.is_cancelled() {
            return Err(ProcessRunError::not_started(ProcessRunFailure::Cancelled));
        }
        let elevated_launch = if elevated {
            if invocation.environment != program::TrustedEnvironment::CurrentUserSystemd
                || invocation.host != crate::app::bootstrap_repair_policy::Host::Linux
                || invocation.arguments.iter().any(|argument| {
                    matches!(
                        argument,
                        super::super::super::TrustedArgument::VerifiedArtifact(_)
                    ) || (!matches!(invocation.program, program::TrustedProgram::Dd)
                        && matches!(
                            argument,
                            super::super::super::TrustedArgument::DockerAptKeyDdInput(_)
                                | super::super::super::TrustedArgument::DockerAptSourcesDdInput(_)
                        ))
                })
            {
                return Err(ProcessRunError::not_started(
                    ProcessRunFailure::FailedBeforeLaunch,
                ));
            }
            Some(scope::prepare(&invocation)?)
        } else {
            None
        };
        let program_path = if let Some(launch) = &elevated_launch {
            launch.systemd_run.clone()
        } else {
            invocation
                .program
                .resolve(invocation.host)
                .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?
        };
        if elevated_launch.is_none() {
            platform::validate_program_path(&program_path)
                .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        }
        let working =
            crate::harness::execution_security::PrivateDirectory::create("koolade-remediation")
                .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        let mut command = Command::new(&program_path);
        if let Some(launch) = &elevated_launch {
            command.args(&launch.arguments);
        } else {
            for argument in &invocation.arguments {
                argument.validate().map_err(|_| {
                    ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch)
                })?;
                command.arg(argument.as_os_string());
            }
        }
        let user_environment =
            if invocation.environment == program::TrustedEnvironment::CurrentUserSystemd {
                Some(user_environment::current_user_systemd().map_err(|_| {
                    ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch)
                })?)
            } else {
                None
            };
        let home = user_environment
            .as_ref()
            .map_or(working.path.as_path(), |environment| {
                environment.home.as_path()
            });
        command
            .env_clear()
            .env("HOME", home)
            .env("TMPDIR", &working.path)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("NO_COLOR", "1")
            .current_dir(&working.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            command.env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
            if let Some(environment) = &user_environment {
                command.env("XDG_RUNTIME_DIR", &environment.runtime_dir);
            } else {
                command
                    .env("XDG_CONFIG_HOME", working.path.join("config"))
                    .env("XDG_DATA_HOME", working.path.join("data"))
                    .env("XDG_CACHE_HOME", working.path.join("cache"));
            }
        }
        #[cfg(windows)]
        {
            let system_directory = program_path.parent().ok_or_else(|| {
                ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch)
            })?;
            command.env("PATH", system_directory).env(
                "SYSTEMROOT",
                system_directory.parent().unwrap_or(system_directory),
            );
        }
        #[cfg(test)]
        if let Some(mode) = &invocation.test_mode {
            command.env("KOOLADE_REMEDIATION_TEST_MODE", mode);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let artifact_fds = invocation.artifact_fds();
            unsafe {
                command.pre_exec(move || {
                    platform::make_artifacts_inheritable(&artifact_fds)?;
                    platform::install_linux_process_group_guard()
                });
            }
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            let artifact_fds = invocation.artifact_fds();
            command.process_group(0);
            unsafe {
                command.pre_exec(move || platform::make_artifacts_inheritable(&artifact_fds));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
        }
        let child = command
            .spawn()
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        monitor::finish(child, &invocation, cancellation, elevated_launch.as_ref())
    }
}
