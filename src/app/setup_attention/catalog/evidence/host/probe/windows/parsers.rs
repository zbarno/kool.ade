use crate::app::setup_attention::catalog::WindowsKernelVersionV1 as KernelVersion;

use super::super::ProbeRead;
use super::classify::OptionalFeatureState;

pub(super) fn optional_feature_state(
    output: &str,
    success: bool,
) -> ProbeRead<OptionalFeatureState> {
    if !success {
        return ProbeRead::Inaccessible;
    }
    let Some(value) = output.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case("State")
            .then(|| value.trim().to_ascii_lowercase())
    }) else {
        return ProbeRead::Malformed;
    };
    match value.as_str() {
        "enabled" => ProbeRead::Found(OptionalFeatureState::Enabled),
        "disabled" => ProbeRead::Found(OptionalFeatureState::Disabled),
        "enable pending" | "disable pending" => ProbeRead::Found(OptionalFeatureState::Pending),
        _ => ProbeRead::Malformed,
    }
}

pub(super) fn parse_kernel_version(output: &str) -> ProbeRead<KernelVersion> {
    let version_text = output
        .lines()
        .find_map(|line| {
            let (label, value) = line.split_once(':')?;
            label
                .trim()
                .eq_ignore_ascii_case("Kernel version")
                .then_some(value.trim())
        })
        .or_else(|| {
            let trimmed = output.trim();
            (!trimmed.is_empty()
                && trimmed
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.'))
            .then_some(trimmed)
        });
    let Some(version_text) = version_text else {
        return if output.is_empty()
            || output.lines().any(|line| {
                line.split_once(':').is_some_and(|(label, _)| {
                    label.trim().eq_ignore_ascii_case("Default Version")
                        || label.trim().eq_ignore_ascii_case("Default Distribution")
                })
            }) {
            ProbeRead::Missing
        } else {
            ProbeRead::Malformed
        };
    };
    if matches!(
        version_text.trim().to_ascii_lowercase().as_str(),
        "not installed" | "not found" | "none"
    ) {
        return ProbeRead::Missing;
    }
    for token in
        version_text.split(|character: char| !character.is_ascii_digit() && character != '.')
    {
        let Some((major, rest)) = token.split_once('.') else {
            continue;
        };
        let mut parts = std::iter::once(major).chain(rest.split('.'));
        let Some(major) = parts.next().and_then(|value| value.parse::<u16>().ok()) else {
            continue;
        };
        let Some(minor) = parts.next().and_then(|value| value.parse::<u16>().ok()) else {
            continue;
        };
        let Some(patch) = parts.next().and_then(|value| value.parse::<u16>().ok()) else {
            continue;
        };
        let build = parts.next().and_then(|value| value.parse::<u16>().ok());
        if major >= 4 && parts.next().is_none() {
            return ProbeRead::Found(KernelVersion {
                major,
                minor,
                patch,
                build,
            });
        }
    }
    if version_text.is_empty() {
        ProbeRead::Missing
    } else {
        ProbeRead::Malformed
    }
}
