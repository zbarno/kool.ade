use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    time::Duration,
};

use windows_sys::Win32::System::{
    SystemInformation::{
        GetNativeSystemInfo, GetSystemDirectoryW, PROCESSOR_ARCHITECTURE_AMD64,
        PROCESSOR_ARCHITECTURE_ARM64, SYSTEM_INFO,
    },
    Threading::IsProcessorFeaturePresent,
};

use crate::{
    app::setup_attention::catalog::{ImageArchitectureV1 as Architecture, WindowsKernelVersionV1},
    harness::discovery::{ProbeOutput, run},
};

use super::super::{ProbeRead, fact};
use super::WindowsFacts;
use super::classify::{
    OptionalFeatureState, edition_from_id, feature_fact, host_fact, kernel_fact, policy_fact,
    product_from_facts, reboot_fact, virtualization_fact,
};
use super::parsers::{optional_feature_state, parse_kernel_version};
use crate::app::setup_attention::catalog::{EvidenceCode as Code, EvidenceState as State};

use self::registry::{pending_reboot, registry_string, wsl_policy};

const PF_VIRT_FIRMWARE_ENABLED: u32 = 21;
const MAX_COMMAND_OUTPUT: usize = 32 * 1024;

pub(super) fn collect() -> WindowsFacts {
    let architecture = native_architecture();
    let edition_read = registry_string(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "EditionID",
    );
    let build_read = registry_string(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "CurrentBuildNumber",
    );
    let host = match (edition_read, build_read) {
        (ProbeRead::Found(edition_id), ProbeRead::Found(build)) => match build.parse::<u32>() {
            Ok(build) => {
                let edition = edition_from_id(&edition_id);
                let product = product_from_facts(edition, build);
                host_fact(product, edition, build, architecture)
            }
            Err(_) => fact(State::Failed, Code::WindowsProbeMalformed, None),
        },
        (ProbeRead::Inaccessible, _) | (_, ProbeRead::Inaccessible) => {
            fact(State::Unknown, Code::WindowsProbeInaccessible, None)
        }
        (ProbeRead::Malformed, _) | (_, ProbeRead::Malformed) => {
            fact(State::Failed, Code::WindowsProbeMalformed, None)
        }
        _ => fact(State::Unknown, Code::WindowsProbeInaccessible, None),
    };

    let wsl_feature = feature_fact(feature_state("Microsoft-Windows-Subsystem-Linux"));
    let vm_platform_feature = feature_fact(feature_state("VirtualMachinePlatform"));
    let wsl_kernel = kernel_fact(kernel_version());
    let mut reboot = pending_reboot();
    if wsl_feature.code == Code::WindowsFeaturePendingReboot
        || vm_platform_feature.code == Code::WindowsFeaturePendingReboot
    {
        reboot = reboot_fact(ProbeRead::Found(true));
    }
    let policy = policy_fact(wsl_policy());
    let virtualization = virtualization_fact(ProbeRead::Found(unsafe {
        IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) != 0
    }));

    WindowsFacts {
        host,
        wsl_feature,
        vm_platform_feature,
        wsl_kernel,
        reboot,
        policy,
        virtualization,
    }
}

fn native_architecture() -> Architecture {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetNativeSystemInfo(&mut info) };
    let architecture = unsafe { info.Anonymous.Anonymous.wProcessorArchitecture };
    match architecture {
        PROCESSOR_ARCHITECTURE_AMD64 => Architecture::Amd64,
        PROCESSOR_ARCHITECTURE_ARM64 => Architecture::Arm64,
        _ => Architecture::Unsupported,
    }
}

fn feature_state(name: &str) -> ProbeRead<OptionalFeatureState> {
    let path = match system_tool("dism.exe") {
        ProbeRead::Found(path) => path,
        ProbeRead::Missing => return ProbeRead::Missing,
        ProbeRead::Inaccessible => return ProbeRead::Inaccessible,
        ProbeRead::Malformed => return ProbeRead::Malformed,
    };
    let arguments = vec![
        "/Online".to_owned(),
        "/Get-FeatureInfo".to_owned(),
        format!("/FeatureName:{name}"),
        "/English".to_owned(),
    ];
    let output = run_system_tool(&path, &arguments);
    match output {
        ProbeRead::Found(output) => {
            optional_feature_state(&String::from_utf8_lossy(&output.stdout), output.success)
        }
        ProbeRead::Missing => ProbeRead::Missing,
        ProbeRead::Inaccessible => ProbeRead::Inaccessible,
        ProbeRead::Malformed => ProbeRead::Malformed,
    }
}

fn kernel_version() -> ProbeRead<WindowsKernelVersionV1> {
    let path = match system_tool("wsl.exe") {
        ProbeRead::Found(path) => path,
        ProbeRead::Missing => return ProbeRead::Missing,
        ProbeRead::Inaccessible => return ProbeRead::Inaccessible,
        ProbeRead::Malformed => return ProbeRead::Malformed,
    };
    match run_system_tool(&path, &["--status".to_owned()]) {
        ProbeRead::Found(output) if output.success => {
            parse_kernel_version(&String::from_utf8_lossy(&output.stdout))
        }
        ProbeRead::Found(_) | ProbeRead::Inaccessible => ProbeRead::Inaccessible,
        ProbeRead::Missing => ProbeRead::Missing,
        ProbeRead::Malformed => ProbeRead::Malformed,
    }
}

fn run_system_tool(path: &Path, arguments: &[String]) -> ProbeRead<ProbeOutput> {
    let Some(publisher) = crate::execution::docker::engine::endpoint::named_pipe::image::signature::
        authenticode_publisher(path)
    else {
        return ProbeRead::Inaccessible;
    };
    if !matches!(
        publisher.as_str(),
        "Microsoft Windows" | "Microsoft Corporation"
    ) {
        return ProbeRead::Malformed;
    }
    let mut argv = vec![path.to_string_lossy().into_owned()];
    argv.extend(arguments.iter().cloned());
    match run(
        &argv,
        Duration::from_secs(20),
        MAX_COMMAND_OUTPUT,
        MAX_COMMAND_OUTPUT,
        &[],
    ) {
        Ok(output) if !output.stdout_truncated && !output.stderr_truncated => {
            ProbeRead::Found(output)
        }
        Ok(_) => ProbeRead::Malformed,
        Err(_) => ProbeRead::Inaccessible,
    }
}

fn system_tool(name: &str) -> ProbeRead<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return ProbeRead::Inaccessible;
    }
    let directory = PathBuf::from(OsString::from_wide(&buffer[..length]));
    let path = directory.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => ProbeRead::Found(path),
        Ok(_) => ProbeRead::Malformed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProbeRead::Missing,
        Err(_) => ProbeRead::Inaccessible,
    }
}

mod registry;
