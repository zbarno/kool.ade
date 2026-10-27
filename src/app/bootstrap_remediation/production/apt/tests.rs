use super::*;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1 as Recipe;

const DOCKER_SOURCES: &str = "Types: deb\nURIs: https://download.docker.com/linux/ubuntu\nSuites: noble\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.asc\n";

// Production boundary: verifies source and candidate validators reject a
// lookalike repository before apt-get can run. These parser tests use only
// synthetic source text and do not access a package manager or network.
#[test]
fn accepts_only_the_exact_active_docker_noble_amd64_source() {
    let inspection = source::inspect_for_test(DOCKER_SOURCES.as_bytes(), true).unwrap();
    assert_eq!(inspection.exact_entries, 1);
    assert!(inspection.has_docker_uri);
    assert!(!inspection.has_invalid_docker_entry);

    for source in [
        DOCKER_SOURCES.replace("Suites: noble", "Suites: jammy"),
        DOCKER_SOURCES.replace("Architectures: amd64", "Architectures: arm64"),
        DOCKER_SOURCES.replace(
            "Signed-By: /etc/apt/keyrings/docker.asc",
            "Signed-By: /tmp/untrusted-key.asc",
        ),
        DOCKER_SOURCES.replace("Components: stable", "Components: test"),
    ] {
        let inspection = source::inspect_for_test(source.as_bytes(), true).unwrap();
        assert_eq!(inspection.exact_entries, 0);
        assert!(inspection.has_invalid_docker_entry);
    }
}

#[test]
fn debian_recipe_requires_the_exact_signed_trixie_sources_and_candidates() {
    let debian_sources = "Types: deb\nURIs: https://download.docker.com/linux/debian\nSuites: trixie\nComponents: stable\nArchitectures: amd64\nSigned-By: /etc/apt/keyrings/docker.asc\n";
    let inspection =
        source::inspect_for_recipe_test(debian_sources.as_bytes(), true, Recipe::Debian13Amd64)
            .unwrap();
    assert_eq!(inspection.exact_entries, 1);
    assert!(!inspection.has_invalid_docker_entry);

    let wrong_release = debian_sources.replace("Suites: trixie", "Suites: bookworm");
    let inspection =
        source::inspect_for_recipe_test(wrong_release.as_bytes(), true, Recipe::Debian13Amd64)
            .unwrap();
    assert!(inspection.has_invalid_docker_entry);

    let docker_candidate = "docker-ce:\n  Installed: (none)\n  Candidate: 5:29.0.0-1~debian.13~trixie\n  Version table:\n     5:29.0.0-1~debian.13~trixie 500\n        500 https://download.docker.com/linux/debian trixie/stable amd64 Packages\n";
    assert!(candidate::candidate_is_from_docker_recipe(
        docker_candidate,
        Recipe::Debian13Amd64
    ));
    assert!(!candidate::candidate_is_from_docker_recipe(
        docker_candidate,
        Recipe::Ubuntu2404Amd64
    ));

    let helper_candidate = "uidmap:\n  Installed: (none)\n  Candidate: 1:4.13+dfsg1-1\n  Version table:\n     1:4.13+dfsg1-1 500\n        500 https://deb.debian.org/debian trixie/main amd64 Packages\n";
    assert!(candidate::candidate_is_from_base_distribution(
        helper_candidate,
        Recipe::Debian13Amd64
    ));
    assert!(!candidate::candidate_is_from_base_distribution(
        &helper_candidate.replace("trixie/main", "bookworm/main"),
        Recipe::Debian13Amd64
    ));
    assert!(!candidate::candidate_is_from_base_distribution(
        &helper_candidate.replace("trixie/main", "trixie/universe"),
        Recipe::Debian13Amd64
    ));
}

#[test]
fn exact_legacy_list_source_requires_arch_and_root_keyring_options() {
    let exact = "deb [arch=amd64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu noble stable\n";
    let inspection = source::inspect_for_test(exact.as_bytes(), false).unwrap();
    assert_eq!(inspection.exact_entries, 1);
    assert!(!inspection.has_invalid_docker_entry);

    for source in [
        "deb https://download.docker.com/linux/ubuntu noble stable\n",
        "deb [arch=arm64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu noble stable\n",
        "deb [arch=amd64 signed-by=/tmp/docker.asc] https://download.docker.com/linux/ubuntu noble stable\n",
        "deb [arch=amd64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu jammy stable\n",
    ] {
        let inspection = source::inspect_for_test(source.as_bytes(), false).unwrap();
        assert_eq!(inspection.exact_entries, 0, "{source}");
        assert!(inspection.has_invalid_docker_entry, "{source}");
    }
}

#[test]
fn ignores_disabled_deb822_docker_entries_but_does_not_count_them() {
    let disabled = format!("{DOCKER_SOURCES}Enabled: no\n");
    let inspection = source::inspect_for_test(disabled.as_bytes(), true).unwrap();
    assert_eq!(inspection.exact_entries, 0);
    assert!(!inspection.has_invalid_docker_entry);
}

#[test]
fn candidate_must_be_from_the_exact_docker_noble_stable_amd64_index() {
    let valid = "docker-ce-rootless-extras:\n  Installed: (none)\n  Candidate: 5:29.0.0-1~ubuntu.24.04~noble\n  Version table:\n     5:29.0.0-1~ubuntu.24.04~noble 500\n        500 https://download.docker.com/linux/ubuntu noble/stable amd64 Packages\n";
    assert!(candidate::candidate_is_from_docker_noble(valid));
    let docker_engine = valid.replace("docker-ce-rootless-extras:", "docker-ce:");
    assert!(candidate::candidate_is_from_docker_noble(&docker_engine));
    let mixed_source = format!(
        "{valid}     5:29.0.0-1~ubuntu.24.04~noble 500\n        500 https://mirror.invalid/packages noble/stable amd64 Packages\n"
    );
    assert!(!candidate::candidate_is_from_docker_noble(&mixed_source));
    for invalid in [
        valid.replace(
            "Candidate: 5:29.0.0-1~ubuntu.24.04~noble",
            "Candidate: (none)",
        ),
        valid.replace("noble/stable", "jammy/stable"),
        valid.replace("amd64 Packages", "arm64 Packages"),
        valid.replace(
            "https://download.docker.com/linux/ubuntu",
            "https://mirror.invalid/linux/ubuntu",
        ),
        valid.replace(
            "Candidate: 5:29.0.0-1~ubuntu.24.04~noble",
            "Candidate: 9.0.0",
        ),
    ] {
        assert!(
            !candidate::candidate_is_from_docker_noble(&invalid),
            "{invalid}"
        );
    }
}

#[test]
fn distro_package_candidates_must_come_from_official_ubuntu_noble_archives() {
    let valid = "uidmap:\n  Installed: (none)\n  Candidate: 1:4.13+dfsg1-1ubuntu1\n  Version table:\n     1:4.13+dfsg1-1ubuntu1 500\n        500 http://archive.ubuntu.com/ubuntu noble/universe amd64 Packages\n";
    assert!(candidate::candidate_is_from_ubuntu_noble(valid));

    let installed = valid.replace(" 1:4.13+dfsg1-1ubuntu1 500", " *** 1:4.13+dfsg1-1ubuntu1 100\n     500 http://archive.ubuntu.com/ubuntu noble/universe amd64 Packages");
    assert!(candidate::candidate_is_from_ubuntu_noble(&installed));

    for invalid in [
        valid.replace("noble/universe", "jammy/universe"),
        valid.replace(
            "http://archive.ubuntu.com/ubuntu",
            "https://mirror.invalid/ubuntu",
        ),
        valid.replace("amd64 Packages", "arm64 Packages"),
        valid.replace("Candidate: 1:4.13+dfsg1-1ubuntu1", "Candidate: (none)"),
    ] {
        assert!(
            !candidate::candidate_is_from_ubuntu_noble(&invalid),
            "{invalid}"
        );
    }
}

#[test]
fn pop_os_helper_candidates_accept_only_stock_noble_repositories() {
    let valid = "uidmap:\n  Installed: (none)\n  Candidate: 1:4.13+dfsg1-1ubuntu1\n  Version table:\n     1:4.13+dfsg1-1ubuntu1 500\n        500 http://apt.pop-os.org/ubuntu noble/universe amd64 Packages\n";
    assert!(candidate::candidate_is_from_base_distribution(
        valid,
        Recipe::PopOs2404Amd64
    ));
    assert!(!candidate::candidate_is_from_base_distribution(
        valid,
        Recipe::Ubuntu2404Amd64
    ));

    let backports = valid.replace("noble/universe", "noble-backports/universe");
    assert!(candidate::candidate_is_from_base_distribution(
        &backports,
        Recipe::PopOs2404Amd64
    ));
    let pop_release = valid
        .replace("apt.pop-os.org/ubuntu", "apt.pop-os.org/release")
        .replace("noble/universe", "noble/main");
    assert!(candidate::candidate_is_from_base_distribution(
        &pop_release,
        Recipe::PopOs2404Amd64
    ));

    for invalid in [
        valid.replace("apt.pop-os.org/ubuntu", "apt.pop-os.org/release"),
        valid.replace("noble/universe", "jammy/universe"),
        valid.replace("apt.pop-os.org/ubuntu", "mirror.invalid/ubuntu"),
    ] {
        assert!(
            !candidate::candidate_is_from_base_distribution(&invalid, Recipe::PopOs2404Amd64),
            "{invalid}"
        );
    }
}

#[test]
fn trusted_apt_metadata_must_be_root_owned_and_not_group_or_world_writable() {
    assert!(source::trusted_root_owner_mode(0, 0o644));
    assert!(source::trusted_root_owner_mode(0, 0o600));
    assert!(!source::trusted_root_owner_mode(1000, 0o644));
    assert!(!source::trusted_root_owner_mode(0, 0o664));
    assert!(!source::trusted_root_owner_mode(0, 0o646));
}

#[test]
fn trusted_apt_directories_and_key_files_remain_accessible_to_apt_sandbox() {
    assert!(source::trusted_root_directory_metadata(0, 0o755));
    assert!(!source::trusted_root_directory_metadata(0, 0o700));
    assert!(!source::trusted_root_directory_metadata(0, 0o775));
    assert!(source::trusted_root_file_metadata(0, 0o644));
    assert!(!source::trusted_root_file_metadata(0, 0o600));
    assert!(!source::trusted_root_file_metadata(0, 0o664));
}

#[test]
fn apt_cache_and_install_options_pin_sources_architecture_and_signature_checks() {
    let expected = [
        "--option=RootDir=/",
        "--option=Dir::Etc::sourcelist=/etc/apt/sources.list",
        "--option=Dir::Etc::sourceparts=/etc/apt/sources.list.d",
        "--option=Dir::State::lists=/var/lib/apt/lists",
        "--option=APT::Get::List-Cleanup=true",
        "--option=APT::Architecture=amd64",
        "--option=APT::Get::AllowUnauthenticated=false",
        "--option=Acquire::AllowInsecureRepositories=false",
        "--option=Acquire::AllowWeakRepositories=false",
        "--option=Acquire::AllowDowngradeToInsecureRepositories=false",
        "--option=Acquire::Check-Date=true",
        "--option=Acquire::Check-Valid-Until=true",
        "--option=Acquire::https::Verify-Peer=true",
        "--option=Acquire::https::Verify-Host=true",
    ];
    let cache_arguments = candidate::apt_cache_arguments_for_test("uidmap");
    let mut expected_cache = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    expected_cache.extend(["policy".to_owned(), "uidmap".to_owned()]);
    assert_eq!(cache_arguments, expected_cache);
    let package_arguments = package_arguments(Recipe::Ubuntu2404Amd64).unwrap();
    let expected_strings = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        package_arguments[..expected.len()]
            .iter()
            .map(|argument| argument.as_os_string().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        expected_strings
    );
}
