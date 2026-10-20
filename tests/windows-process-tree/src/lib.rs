#![allow(dead_code)]

mod harness {
    pub(crate) mod execution_security {
        #[derive(Clone, Copy)]
        pub(crate) enum CliProvider {
            Antigravity,
            Claude,
            Codex,
            Copilot,
            OpenCode,
            Pi,
        }
    }
}

#[cfg(windows)]
#[path = "../../../src/harness/discovery/windows.rs"]
mod windows;

#[cfg(windows)]
#[path = "../../../src/harness/pi_proc/launch_environment.rs"]
mod launch_environment;
