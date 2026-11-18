#![cfg(unix)]

mod computer_use {
    pub mod backend {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ComputerHostPlatformV1 {
            LinuxX86_64,
            MacOsIntel,
            MacOsAppleSilicon,
            #[allow(dead_code)]
            Unsupported,
        }

        impl ComputerHostPlatformV1 {
            pub fn current() -> Self {
                Self::LinuxX86_64
            }
        }
    }

    pub(crate) mod macos_backend {
        use crate::computer_use::backend::ComputerHostPlatformV1;
        pub(crate) use crate::qemu_plan;

        pub(crate) struct MacOsTrustedQemuRuntimeV1 {
            path: std::path::PathBuf,
            platform: ComputerHostPlatformV1,
        }

        impl MacOsTrustedQemuRuntimeV1 {
            pub(crate) fn fixture(
                path: std::path::PathBuf,
                platform: ComputerHostPlatformV1,
            ) -> Self {
                Self { path, platform }
            }

            pub(crate) fn path_for_plan(
                &self,
                plan: &crate::qemu_plan::MacOsQemuLaunchPlanV1,
            ) -> std::io::Result<&std::path::Path> {
                if self.platform == plan.platform
                    && self.path.file_name().and_then(|name| name.to_str()) == Some(plan.executable)
                {
                    Ok(&self.path)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "runtime fixture does not match the plan",
                    ))
                }
            }
        }

        pub(crate) struct MacOsHvfReadiness {
            pub(crate) ready_for_adapter: bool,
            pub(crate) blocker: Option<()>,
            pub(crate) release_enabled: bool,
        }

        pub(crate) fn probe_macos_hvf() -> MacOsHvfReadiness {
            MacOsHvfReadiness {
                ready_for_adapter: false,
                blocker: Some(()),
                release_enabled: false,
            }
        }
    }
}

#[path = "../src/computer_use/macos_backend/process.rs"]
mod process;
#[path = "../src/computer_use/macos_backend/qemu_plan.rs"]
pub(crate) mod qemu_plan;
