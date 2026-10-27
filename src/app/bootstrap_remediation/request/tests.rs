use super::*;

fn image_request() -> RemediationRequest {
    let identity =
        HostIdentity::from_facts(Host::Linux, "6.12.1", "test-machine", "test-user").unwrap();
    RemediationRequest::from_proposal_for_test(
        &Proposal {
            schema_version: 1,
            operation: Operation::PrepareLockedImage,
            host: Host::Linux,
            prerequisite_id: format!("setup:images:general:amd64:{}", "a".repeat(64)),
            image_profile: Some("general".into()),
        },
        &identity,
    )
    .unwrap()
}

#[test]
fn strict_request_round_trip_rejects_commands_and_unknown_parameters() {
    let request = image_request();
    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(
        serde_json::from_str::<RemediationRequest>(&encoded).unwrap(),
        request
    );
    assert_eq!(
        request.parameters,
        OperationParameters::LockedImage {
            profile: "general".into(),
            architecture: "amd64".into(),
        }
    );
    assert_eq!(request.proposal().prerequisite_id, request.prerequisite_id);
    let injected = encoded.replace(
        "\"kind\":\"locked_image\"",
        "\"kind\":\"locked_image\",\"script\":\"sudo anything\"",
    );
    assert!(serde_json::from_str::<RemediationRequest>(&injected).is_err());
    assert!(serde_json::from_str::<RemediationRequest>(
        r#"{"schemaVersion":1,"operation":"run_host_command","host":"linux","osVersion":"6.12","prerequisiteId":"setup:docker","parameters":{"kind":"none"}}"#
    )
    .is_err());
}

#[test]
fn parameters_host_and_os_version_are_revalidated() {
    let mut request = image_request();
    request.parameters = OperationParameters::None;
    assert!(request.validate_shape().is_err());
    let mut request = image_request();
    request.os_version = "6.12\nmalicious".into();
    assert!(request.validate_shape().is_err());
    let mut request = image_request();
    request.prerequisite_id = "setup:../../escape".into();
    assert!(request.validate_shape().is_err());
    let mut request = image_request();
    if let OperationParameters::LockedImage { architecture, .. } = &mut request.parameters {
        *architecture = "arm64".into();
    }
    assert!(request.validate_shape().is_err());
}

#[test]
fn request_requires_verified_current_host_and_os_version() {
    let request = image_request();
    assert!(
        request
            .validate_for_identity(&HostIdentity::current().unwrap())
            .is_err()
    );
}

#[test]
fn rootless_operations_reject_kernels_below_the_supported_floor() {
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::InstallLinuxRootlessPackages,
        host: Host::Linux,
        prerequisite_id: "setup:linux:rootless:packages".into(),
        image_profile: None,
    };
    for kernel in ["4.14.999-1-generic", "4.9.250", "malformed"] {
        let identity = HostIdentity::from_facts(
            Host::Linux,
            &format!("ubuntu-24.04-amd64-kernel-{kernel}"),
            "test-machine",
            "test-user",
        )
        .unwrap();
        assert!(
            RemediationRequest::from_proposal_for_test(&proposal, &identity).is_err(),
            "kernel {kernel} unexpectedly passed"
        );
    }
    for kernel in ["4.15.0-1-generic", "5.4.0-1-generic", "6.8.0-31-generic"] {
        let identity = HostIdentity::from_facts(
            Host::Linux,
            &format!("ubuntu-24.04-amd64-kernel-{kernel}"),
            "test-machine",
            "test-user",
        )
        .unwrap();
        assert!(
            RemediationRequest::from_proposal_for_test(&proposal, &identity).is_ok(),
            "kernel {kernel} should meet the supported floor"
        );
    }
}

#[test]
fn rootless_requests_bind_each_exact_distro_recipe_and_reject_lookalikes() {
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::InstallLinuxRootlessPackages,
        host: Host::Linux,
        prerequisite_id: "setup:linux:rootless:packages".into(),
        image_profile: None,
    };
    for (os_version, expected_recipe) in [
        (
            "debian-13-amd64-kernel-6.12.1",
            LinuxRootlessRecipeV1::Debian13Amd64,
        ),
        (
            "ubuntu-24.04-amd64-kernel-6.8.0",
            LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        ),
        (
            "pop-24.04-amd64-kernel-6.17.9",
            LinuxRootlessRecipeV1::PopOs2404Amd64,
        ),
        (
            "fedora-45-amd64-kernel-6.17.9",
            LinuxRootlessRecipeV1::Fedora45Amd64,
        ),
    ] {
        let identity =
            HostIdentity::from_facts(Host::Linux, os_version, "test-machine", "test-user").unwrap();
        let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
        assert!(matches!(
            request.parameters,
            OperationParameters::InstallLinuxRootlessPackages { recipe }
                if recipe == expected_recipe
        ));
    }

    let unsupported = HostIdentity::from_facts(
        Host::Linux,
        "debian-12-amd64-kernel-6.12.1",
        "test-machine",
        "test-user",
    )
    .unwrap();
    assert!(RemediationRequest::from_proposal_for_test(&proposal, &unsupported).is_err());

    let identity = HostIdentity::from_facts(
        Host::Linux,
        "fedora-45-amd64-kernel-6.17.9",
        "test-machine",
        "test-user",
    )
    .unwrap();
    let mut request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    request.parameters = OperationParameters::InstallLinuxRootlessPackages {
        recipe: LinuxRootlessRecipeV1::Ubuntu2404Amd64,
    };
    assert!(request.validate_shape().is_err());
}
