use super::*;

fn account() -> CurrentAccount {
    CurrentAccount {
        uid: 1000,
        name: "synthetic-user".into(),
    }
}

// Production boundary: checks every parsed /etc/subuid and /etc/subgid range
// before usermod receives the selected mapping. Fixtures are synthetic; no
// account files or privileged operation are changed.
#[test]
fn permits_only_clean_non_overlapping_files_without_current_user_mappings() {
    validate_mapping_file("other-user:100000:65536\n", &account(), "subuid").unwrap();
    validate_mapping_file("other-group:400000:65536\n", &account(), "subgid").unwrap();
    validate_mapping_file("# comment\n\n", &account(), "subuid").unwrap();
}

#[test]
fn refuses_existing_current_user_mapping_even_when_only_one_file_has_it() {
    for owner in ["synthetic-user", "1000"] {
        assert!(
            validate_mapping_file(&format!("{owner}:400000:65536\n"), &account(), "subuid")
                .is_err()
        );
        assert!(
            validate_mapping_file(&format!("{owner}:400000:65536\n"), &account(), "subgid")
                .is_err()
        );
        assert!(
            validate_mapping_file(&format!("{owner}:400000:1024\n"), &account(), "subuid").is_err()
        );
        assert!(
            validate_mapping_file(&format!("{owner}:400000:1024\n"), &account(), "subgid").is_err()
        );
    }
}

#[test]
fn refuses_partial_malformed_overflow_and_conflicting_mappings() {
    for contents in [
        "synthetic-user:231072\n",
        "other-user:broken:65536\n",
        "other-user:231072:65536:extra\n",
        "other-user:4294967290:20\n",
        "other-user:231072:65536\n",
        "one-user:400000:65536\ntwo-user:450000:65536\n",
    ] {
        assert!(
            validate_mapping_file(contents, &account(), "subuid").is_err(),
            "{contents}"
        );
    }
}

#[test]
fn fixed_docker_documented_65k_mapping_is_bound_to_current_account() {
    let args = arguments_for(&account());
    let invocation = crate::app::bootstrap_remediation::TrustedInvocation::new(
        crate::app::bootstrap_repair_policy::Host::Linux,
        crate::app::bootstrap_remediation::TrustedProgram::Usermod,
        args,
        std::time::Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(
        invocation.arguments_for_test(),
        [
            "--add-subuids",
            "231072-296607",
            "--add-subgids",
            "231072-296607",
            "--",
            "synthetic-user",
        ]
    );
    assert_eq!(RANGE_COUNT, 65_536);
    assert_eq!(RANGE_END - RANGE_START + 1, RANGE_COUNT);
}
