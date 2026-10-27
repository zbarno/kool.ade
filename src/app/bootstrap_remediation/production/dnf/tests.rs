use super::*;
use crate::app::bootstrap_remediation::trusted_assets::{
    DOCKER_DNF_KEY, DOCKER_DNF_KEY_FINGERPRINT, DOCKER_DNF_KEY_SHA256, DOCKER_DNF_REPO,
    DOCKER_DNF_REPO_SHA256,
};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;
use sha2::{Digest, Sha256};

const APPROVED_CANDIDATES: &str = "docker-ce|koolade-docker-ce-stable\n\
    docker-ce-cli|koolade-docker-ce-stable\n\
    containerd.io|koolade-docker-ce-stable\n\
    docker-ce-rootless-extras|koolade-docker-ce-stable\n\
    shadow-utils|updates\n\
    slirp4netns|fedora\n";

#[test]
fn bundled_fedora_key_and_repo_match_pinned_identity_and_content() {
    assert_eq!(
        format!("{:x}", Sha256::digest(DOCKER_DNF_KEY)),
        DOCKER_DNF_KEY_SHA256
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(DOCKER_DNF_REPO)),
        DOCKER_DNF_REPO_SHA256
    );
    assert!(
        std::str::from_utf8(DOCKER_DNF_REPO)
            .unwrap()
            .contains("gpgcheck=1\n")
    );
    key::verify_docker_signing_key(DOCKER_DNF_KEY).unwrap();
    assert_eq!(
        DOCKER_DNF_KEY_FINGERPRINT,
        "060A61C51B558A7F742B77AAC52FEB6B621E9F35"
    );
    let mut altered_key = DOCKER_DNF_KEY.to_vec();
    altered_key[0] = b'x';
    assert!(key::verify_docker_signing_key(&altered_key).is_err());
}

#[test]
fn candidate_origins_require_exact_vendor_and_fedora_repo_ids() {
    assert!(candidate::candidates_have_approved_origins(
        APPROVED_CANDIDATES
    ));
    assert!(!candidate::candidates_have_approved_origins(
        &APPROVED_CANDIDATES.replace("shadow-utils|updates", "shadow-utils|third-party")
    ));
    assert!(!candidate::candidates_have_approved_origins(
        &APPROVED_CANDIDATES.replace("docker-ce|koolade-docker-ce-stable", "docker-ce|fedora")
    ));
}

#[test]
fn candidate_origins_reject_missing_unexpected_and_malformed_rows() {
    assert!(!candidate::candidates_have_approved_origins(
        &APPROVED_CANDIDATES.replace("slirp4netns|fedora\n", "")
    ));
    assert!(!candidate::candidates_have_approved_origins(&format!(
        "{APPROVED_CANDIDATES}other-package|fedora\n"
    )));
    assert!(!candidate::candidates_have_approved_origins(&format!(
        "{APPROVED_CANDIDATES}unparsed output\n"
    )));
    assert!(!candidate::candidates_have_approved_origins(&format!(
        "{APPROVED_CANDIDATES}docker-ce|koolade-docker-ce-stable\n"
    )));
}

#[test]
fn dnf_common_arguments_pin_cache_tls_trust_and_repository_availability() {
    let arguments = candidate::common_arguments()
        .iter()
        .map(|argument| argument.as_os_string().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for expected in [
        "--setopt=cachedir=/var/cache/libdnf5",
        "--setopt=system_cachedir=/var/cache/libdnf5",
        "--setopt=sslverify=1",
        "--setopt=sslcacert=",
        "--setopt=proxy_sslverify=1",
        "--setopt=proxy_sslcacert=",
        "--setopt=skip_if_unavailable=False",
        "--setopt=fedora.sslverify=1",
        "--setopt=fedora.sslcacert=",
        "--setopt=fedora.proxy_sslverify=1",
        "--setopt=fedora.proxy_sslcacert=",
        "--setopt=fedora.skip_if_unavailable=False",
        "--setopt=updates.sslverify=1",
        "--setopt=updates.sslcacert=",
        "--setopt=updates.proxy_sslverify=1",
        "--setopt=updates.proxy_sslcacert=",
        "--setopt=updates.skip_if_unavailable=False",
        "--setopt=koolade-docker-ce-stable.sslverify=1",
        "--setopt=koolade-docker-ce-stable.sslcacert=",
        "--setopt=koolade-docker-ce-stable.proxy_sslverify=1",
        "--setopt=koolade-docker-ce-stable.proxy_sslcacert=",
        "--setopt=koolade-docker-ce-stable.skip_if_unavailable=False",
    ] {
        assert_eq!(
            arguments
                .iter()
                .filter(|argument| argument.as_str() == expected)
                .count(),
            1
        );
    }
}

#[test]
fn package_backend_api_is_bound_to_the_fedora45_recipe() {
    assert!(package_arguments(LinuxRootlessRecipeV1::Fedora45Amd64).is_ok());
    assert!(package_arguments(LinuxRootlessRecipeV1::Ubuntu2404Amd64).is_err());
    assert!(validate_package_source_bootstrap(LinuxRootlessRecipeV1::Debian13Amd64).is_err());
}
