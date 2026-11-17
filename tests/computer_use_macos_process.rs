#![cfg(unix)]

mod computer_use {
    pub mod backend {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ComputerHostPlatformV1 {
            LinuxX86_64,
            MacOsIntel,
            MacOsAppleSilicon,
        }
    }

    pub(crate) mod macos_backend {
        pub(crate) use crate::qemu_plan;
    }
}

#[path = "../src/computer_use/macos_backend/process.rs"]
mod process;
#[path = "../src/computer_use/macos_backend/qemu_plan.rs"]
pub(crate) mod qemu_plan;
