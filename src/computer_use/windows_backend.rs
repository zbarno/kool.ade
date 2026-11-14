//! Windows x64 host prerequisite and VM-process containment primitives.
//!
//! This is not a certified ComputerBackend. The release gate remains closed
//! until the integrated #392 Windows host matrix is accepted.

use super::ComputerHostPlatformV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowsWpxBlocker {
    UnsupportedArchitecture,
    UnsupportedBuild,
    HypervisorPlatformMissing,
    HypervisorUnavailable,
    ProbeUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowsWpxReadiness {
    pub(crate) ready_for_adapter: bool,
    pub(crate) blocker: Option<WindowsWpxBlocker>,
    pub(crate) release_enabled: bool,
}

pub(crate) fn assess_windows_wpx(
    platform: ComputerHostPlatformV1,
    build: Option<u32>,
    api_present: Option<bool>,
    hypervisor_present: Option<bool>,
) -> WindowsWpxReadiness {
    let blocker = if platform != ComputerHostPlatformV1::WindowsX64 {
        Some(WindowsWpxBlocker::UnsupportedArchitecture)
    } else if build.is_none() {
        Some(WindowsWpxBlocker::ProbeUnavailable)
    } else if !build.is_some_and(|build| build >= 19_041) {
        Some(WindowsWpxBlocker::UnsupportedBuild)
    } else {
        match api_present {
            None => Some(WindowsWpxBlocker::ProbeUnavailable),
            Some(false) => Some(WindowsWpxBlocker::HypervisorPlatformMissing),
            Some(true) => match hypervisor_present {
                None => Some(WindowsWpxBlocker::ProbeUnavailable),
                Some(false) => Some(WindowsWpxBlocker::HypervisorUnavailable),
                Some(true) => None,
            },
        }
    };
    WindowsWpxReadiness {
        ready_for_adapter: blocker.is_none(),
        blocker,
        // Host prerequisites do not certify the adapter or open release support.
        release_enabled: false,
    }
}

#[cfg(windows)]
pub(crate) fn probe_windows_wpx() -> WindowsWpxReadiness {
    let platform = ComputerHostPlatformV1::current();
    if platform != ComputerHostPlatformV1::WindowsX64 {
        return assess_windows_wpx(platform, None, None, None);
    }
    let Some(build) = windows_build_number() else {
        return assess_windows_wpx(platform, None, None, None);
    };
    if build < 19_041 {
        return assess_windows_wpx(platform, Some(build), None, None);
    }
    match query_wpx_hypervisor() {
        NativeWpxProbe::ApiMissing => assess_windows_wpx(platform, Some(build), Some(false), None),
        NativeWpxProbe::ProbeFailed => assess_windows_wpx(platform, Some(build), Some(true), None),
        NativeWpxProbe::HypervisorPresent(present) => {
            assess_windows_wpx(platform, Some(build), Some(true), Some(present))
        }
    }
}

#[cfg(windows)]
fn windows_build_number() -> Option<u32> {
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }
    let mut info = OsVersionInfo {
        size: std::mem::size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform_id: 0,
        service_pack: [0; 128],
    };
    // SAFETY: `info` has the documented RTL_OSVERSIONINFOW layout and size.
    (unsafe { RtlGetVersion(&mut info) } == 0).then_some(info.build)
}

#[cfg(windows)]
enum NativeWpxProbe {
    ApiMissing,
    ProbeFailed,
    HypervisorPresent(bool),
}

#[cfg(windows)]
fn query_wpx_hypervisor() -> NativeWpxProbe {
    use std::{ffi::c_void, mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::FreeLibrary,
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW},
    };

    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
    let dll = "WinHvPlatform.dll"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: the search flag restricts loading to the trusted System32 directory.
    let module = unsafe { LoadLibraryExW(dll.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return NativeWpxProbe::ApiMissing;
    }
    // SAFETY: the symbol is an exported API from WinHvPlatform.dll.
    let symbol = unsafe { GetProcAddress(module, c"WHvGetCapability".as_ptr().cast()) };
    let Some(symbol) = symbol else {
        // SAFETY: `module` is a live handle returned by LoadLibraryExW.
        unsafe { FreeLibrary(module) };
        return NativeWpxProbe::ApiMissing;
    };
    type GetCapability = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
    // SAFETY: WHvGetCapability has this ABI and signature in WinHvPlatform.h.
    let get_capability: GetCapability = unsafe { std::mem::transmute(symbol) };
    let mut hypervisor_present = 0_i32;
    let mut written = 0_u32;
    // SAFETY: a correctly sized BOOL output buffer is supplied for capability 0.
    let result = unsafe {
        get_capability(
            0,
            (&mut hypervisor_present as *mut i32).cast(),
            size_of::<i32>() as u32,
            &mut written,
        )
    };
    // SAFETY: `module` is a live handle returned by LoadLibraryExW.
    unsafe { FreeLibrary(module) };
    if result != 0 || written != size_of::<i32>() as u32 {
        NativeWpxProbe::ProbeFailed
    } else {
        NativeWpxProbe::HypervisorPresent(hypervisor_present != 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowsVmJobLimits {
    #[cfg_attr(not(windows), allow(dead_code))]
    process_memory_bytes: usize,
    #[cfg_attr(not(windows), allow(dead_code))]
    cpu_rate_hundredths_percent: u32,
    #[cfg_attr(not(windows), allow(dead_code))]
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

#[cfg(windows)]
mod process;

#[cfg(test)]
mod tests {
    use super::*;

    fn wpx(
        platform: ComputerHostPlatformV1,
        build: Option<u32>,
        api: Option<bool>,
        hypervisor: Option<bool>,
    ) -> WindowsWpxReadiness {
        assess_windows_wpx(platform, build, api, hypervisor)
    }

    #[test]
    fn whpx_prerequisites_fail_closed_by_architecture_build_and_probe_state() {
        for (platform, build, api, hypervisor, blocker) in [
            (
                ComputerHostPlatformV1::WindowsArm64,
                Some(22_000),
                Some(true),
                Some(true),
                WindowsWpxBlocker::UnsupportedArchitecture,
            ),
            (
                ComputerHostPlatformV1::WindowsX64,
                None,
                None,
                None,
                WindowsWpxBlocker::ProbeUnavailable,
            ),
            (
                ComputerHostPlatformV1::WindowsX64,
                Some(19_039),
                Some(true),
                Some(true),
                WindowsWpxBlocker::UnsupportedBuild,
            ),
            (
                ComputerHostPlatformV1::WindowsX64,
                Some(19_041),
                Some(false),
                None,
                WindowsWpxBlocker::HypervisorPlatformMissing,
            ),
            (
                ComputerHostPlatformV1::WindowsX64,
                Some(19_041),
                Some(true),
                Some(false),
                WindowsWpxBlocker::HypervisorUnavailable,
            ),
            (
                ComputerHostPlatformV1::WindowsX64,
                Some(19_041),
                None,
                Some(true),
                WindowsWpxBlocker::ProbeUnavailable,
            ),
        ] {
            let readiness = wpx(platform, build, api, hypervisor);
            assert!(!readiness.ready_for_adapter);
            assert!(!readiness.release_enabled);
            assert_eq!(readiness.blocker, Some(blocker));
        }
    }

    #[test]
    fn whpx_prerequisites_never_open_the_release_gate() {
        let readiness = wpx(
            ComputerHostPlatformV1::WindowsX64,
            Some(19_041),
            Some(true),
            Some(true),
        );
        assert!(readiness.ready_for_adapter);
        assert_eq!(readiness.blocker, None);
        assert!(!readiness.release_enabled);
    }

    #[test]
    fn vm_job_limits_reject_unbounded_or_invalid_values() {
        assert!(WindowsVmJobLimits::new(0, 5_000, 1).is_none());
        assert!(WindowsVmJobLimits::new(1 << 30, 0, 1).is_none());
        assert!(WindowsVmJobLimits::new(1 << 30, 10_001, 1).is_none());
        assert!(WindowsVmJobLimits::new(1 << 30, 5_000, 0).is_none());
        assert!(WindowsVmJobLimits::new(1 << 30, 5_000, 65).is_none());
        assert!(WindowsVmJobLimits::new(1 << 30, 5_000, 1).is_some());
    }
}

#[cfg(all(test, windows))]
mod native_tests {
    #[test]
    fn native_wpx_probe_keeps_unverified_hosts_unsupported() {
        let probe = super::probe_windows_wpx();
        assert!(!probe.release_enabled);
        if probe.blocker.is_some() {
            assert!(!probe.ready_for_adapter);
        }
    }
}
