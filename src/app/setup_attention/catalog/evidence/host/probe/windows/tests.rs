use super::super::ProbeRead;
use super::classify::*;
use super::parsers::{optional_feature_state, parse_kernel_version};
use crate::app::setup_attention::catalog::{
    EvidenceState as State, ImageArchitectureV1 as Architecture, WindowsEditionV1 as Edition,
    WindowsKernelVersionV1 as KernelVersion, WindowsProductV1 as Product,
};

#[test]
fn edition_and_product_classification_cover_supported_and_unknown_hosts() {
    assert_eq!(edition_from_id("Core"), Edition::Home);
    assert_eq!(edition_from_id("Professional"), Edition::Pro);
    assert_eq!(edition_from_id("Enterprise"), Edition::Enterprise);
    assert_eq!(edition_from_id("ServerStandard"), Edition::Server);
    assert_eq!(edition_from_id("custom-edition"), Edition::Other);
    assert_eq!(
        product_from_facts(Edition::Home, 22_621),
        Product::Windows11
    );
    assert_eq!(product_from_facts(Edition::Pro, 19_045), Product::Windows10);
    assert_eq!(product_from_facts(Edition::Pro, 10_240), Product::Windows10);
    assert_eq!(
        product_from_facts(Edition::Pro, 10_239),
        Product::Unsupported
    );
    assert_eq!(product_from_facts(Edition::Server, 20_000), Product::Server);
}

#[test]
fn host_readiness_requires_supported_edition_build_and_architecture() {
    assert_eq!(
        host_fact(
            Product::Windows11,
            Edition::Home,
            22_000,
            Architecture::Amd64
        )
        .state,
        State::Ready
    );
    assert_eq!(
        host_fact(
            Product::Windows10,
            Edition::Pro,
            19_041,
            Architecture::Amd64
        )
        .state,
        State::Unsupported
    );
    assert_eq!(
        host_fact(
            Product::Windows11,
            Edition::Pro,
            22_000,
            Architecture::Arm64
        )
        .state,
        State::Unsupported
    );
    assert_eq!(
        host_fact(
            Product::Windows10,
            Edition::Pro,
            18_362,
            Architecture::Amd64
        )
        .state,
        State::Unsupported
    );
    assert_eq!(
        host_fact(
            Product::Windows11,
            Edition::Server,
            22_000,
            Architecture::Amd64
        )
        .state,
        State::Unsupported
    );
    assert_eq!(
        host_fact(
            Product::Windows11,
            Edition::Other,
            22_000,
            Architecture::Amd64
        )
        .state,
        State::Unknown
    );
    assert_eq!(
        host_fact(
            Product::Windows11,
            Edition::Pro,
            22_000,
            Architecture::Unsupported
        )
        .state,
        State::Unsupported
    );
}

#[test]
fn dism_feature_states_are_parsed_without_accepting_unknown_output() {
    assert_eq!(
        optional_feature_state("State : Enabled", true),
        ProbeRead::Found(OptionalFeatureState::Enabled)
    );
    assert_eq!(
        optional_feature_state("State : Disabled", true),
        ProbeRead::Found(OptionalFeatureState::Disabled)
    );
    assert_eq!(
        optional_feature_state("State : Enable Pending", true),
        ProbeRead::Found(OptionalFeatureState::Pending)
    );
    assert_eq!(
        optional_feature_state("State : Disable Pending", true),
        ProbeRead::Found(OptionalFeatureState::Pending)
    );
    assert_eq!(
        optional_feature_state("State : Unknown", true),
        ProbeRead::Malformed
    );
    assert_eq!(optional_feature_state("", true), ProbeRead::Malformed);
    assert_eq!(
        optional_feature_state("State : Enabled", false),
        ProbeRead::Inaccessible
    );
}

#[test]
fn kernel_parser_accepts_kernel_version_and_rejects_unavailable_output() {
    assert_eq!(
        parse_kernel_version("Default Distribution: Ubuntu\nKernel version: 5.15.153.1"),
        ProbeRead::Found(KernelVersion {
            major: 5,
            minor: 15,
            patch: 153,
            build: Some(1)
        })
    );
    assert_eq!(
        parse_kernel_version("Kernel version: 5.15.153"),
        ProbeRead::Found(KernelVersion {
            major: 5,
            minor: 15,
            patch: 153,
            build: None
        })
    );
    assert_eq!(
        parse_kernel_version("WSL version: 2.0.0"),
        ProbeRead::Malformed
    );
    assert_eq!(
        parse_kernel_version("Default Version: 2"),
        ProbeRead::Missing
    );
    assert_eq!(
        parse_kernel_version("Kernel version: Not installed"),
        ProbeRead::Missing
    );
    assert_eq!(parse_kernel_version(""), ProbeRead::Missing);
}

#[test]
fn policy_virtualization_and_reboot_states_preserve_fail_closed_results() {
    assert_eq!(policy_fact(ProbeRead::Missing).state, State::Ready);
    assert_eq!(
        policy_fact(ProbeRead::Found(Some(false))).state,
        State::Blocked
    );
    assert_eq!(policy_fact(ProbeRead::Inaccessible).state, State::Unknown);
    assert_eq!(
        virtualization_fact(ProbeRead::Found(false)).state,
        State::Missing
    );
    assert_eq!(reboot_fact(ProbeRead::Found(true)).state, State::Missing);
    assert_eq!(reboot_fact(ProbeRead::Found(false)).state, State::Ready);
}
