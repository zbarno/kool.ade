use super::*;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

#[test]
fn bundled_docker_key_and_source_match_the_approved_recipe() {
    verify_docker_signing_key(DOCKER_APT_KEY).unwrap();
    let inspection = inspect_source_contents(
        DOCKER_APT_SOURCES,
        true,
        LinuxRootlessRecipeV1::Ubuntu2404Amd64,
    )
    .unwrap();
    assert_eq!(inspection.exact_entries, 1);
    assert!(!inspection.has_invalid_docker_entry);

    let (key, sources) = docker_source_file_artifacts(
        LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        DockerSourceBootstrap {
            create_keyrings_directory: true,
            install_key: true,
            install_sources: true,
        },
    )
    .unwrap();
    key.unwrap().validate().unwrap();
    sources.unwrap().validate().unwrap();
}

#[test]
fn debian_source_artifact_is_pinned_to_the_trixie_recipe() {
    let inspection = inspect_source_contents(
        DOCKER_DEBIAN_SOURCES,
        true,
        LinuxRootlessRecipeV1::Debian13Amd64,
    )
    .unwrap();
    assert_eq!(inspection.exact_entries, 1);
    assert!(!inspection.has_invalid_docker_entry);

    let (_, sources) = docker_source_file_artifacts(
        LinuxRootlessRecipeV1::Debian13Amd64,
        DockerSourceBootstrap {
            create_keyrings_directory: false,
            install_key: false,
            install_sources: true,
        },
    )
    .unwrap();
    sources.unwrap().validate().unwrap();
}

#[test]
fn apt_list_sources_cannot_disable_archive_signature_verification() {
    for option in [
        "trusted=yes",
        "allow-insecure=yes",
        "allow-weak=yes",
        "allow-downgrade-to-insecure=yes",
    ] {
        let source =
            format!("deb [arch=amd64 {option}] http://archive.ubuntu.com/ubuntu noble main\n");
        assert!(
            inspect_source_contents(
                source.as_bytes(),
                false,
                LinuxRootlessRecipeV1::Ubuntu2404Amd64,
            )
            .is_err(),
            "accepted unsafe APT source option {option}"
        );
    }
}

#[test]
fn apt_deb822_sources_cannot_disable_archive_signature_verification() {
    for (field, value) in [
        ("Trusted", "yes"),
        ("Allow-Insecure", "yes"),
        ("Allow-Weak", "yes"),
        ("Allow-Downgrade-To-Insecure", "yes"),
    ] {
        let source = format!(
            "Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble\nComponents: main\n{field}: {value}\n"
        );
        assert!(
            inspect_source_contents(
                source.as_bytes(),
                true,
                LinuxRootlessRecipeV1::Ubuntu2404Amd64,
            )
            .is_err(),
            "accepted unsafe Deb822 APT field {field}"
        );
    }
    let continued = b"Types: deb\nURIs: http://archive.ubuntu.com/ubuntu\nSuites: noble\nComponents: main\nTrusted:\n yes\n";
    assert!(
        inspect_source_contents(continued, true, LinuxRootlessRecipeV1::Ubuntu2404Amd64,).is_err(),
        "accepted a continued Deb822 Trusted field"
    );
}
