#![cfg(windows)]
#![allow(dead_code)]

mod computer_use {
    pub(crate) mod backend {
        use std::io::{self, Read, Write};
        use std::time::Duration;

        pub(crate) trait GuestChannelIo: Read + Write + Send {
            fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
            fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
        }
    }

    pub(crate) struct ComputerGuestChannelV1 {
        _stream: Box<dyn backend::GuestChannelIo>,
    }

    impl ComputerGuestChannelV1 {
        pub(crate) fn from_io(
            _session_id: String,
            _lease_generation: u64,
            stream: impl backend::GuestChannelIo + 'static,
        ) -> Self {
            Self {
                _stream: Box::new(stream),
            }
        }
    }

    pub(crate) mod windows_backend {
        #[derive(Clone, Copy)]
        pub(crate) struct WindowsWpxReadiness {
            pub(crate) ready_for_adapter: bool,
            pub(crate) release_enabled: bool,
            pub(crate) blocker: Option<&'static str>,
        }

        pub(crate) fn probe_windows_wpx() -> WindowsWpxReadiness {
            WindowsWpxReadiness {
                ready_for_adapter: false,
                release_enabled: false,
                blocker: Some("test harness does not certify WHPX readiness"),
            }
        }

        #[derive(Clone, Copy)]
        pub(crate) struct WindowsVmJobLimits {
            process_memory_bytes: usize,
            cpu_rate_hundredths_percent: u32,
            active_processes: u32,
        }

        impl WindowsVmJobLimits {
            pub(crate) fn new(
                process_memory_bytes: u64,
                cpu_rate_hundredths_percent: u32,
                active_processes: u32,
            ) -> Option<Self> {
                let process_memory_bytes = usize::try_from(process_memory_bytes).ok()?;
                (process_memory_bytes > 0
                    && (1..=10_000).contains(&cpu_rate_hundredths_percent)
                    && (1..=64).contains(&active_processes))
                .then_some(Self {
                    process_memory_bytes,
                    cpu_rate_hundredths_percent,
                    active_processes,
                })
            }
        }

        #[path = "../../../../../src/computer_use/windows_backend/process.rs"]
        mod process;
        #[path = "../../../../../src/computer_use/windows_backend/storage_policy.rs"]
        mod storage_policy;

        #[path = "../../../../../src/computer_use/windows_backend/storage.rs"]
        mod storage;

        #[path = "../../../../../src/computer_use/windows_backend/transport.rs"]
        mod transport;
    }
}
