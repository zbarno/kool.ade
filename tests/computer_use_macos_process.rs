#![cfg(unix)]

mod computer_use {
    pub mod backend {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ComputerHostPlatformV1 {
            LinuxX86_64,
            MacOsIntel,
            MacOsAppleSilicon,
        }

        impl ComputerHostPlatformV1 {
            pub fn current() -> Self {
                Self::LinuxX86_64
            }
        }
    }

    pub(crate) mod macos_backend {
        pub(crate) use crate::qemu_plan;

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
