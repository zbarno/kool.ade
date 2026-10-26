use super::*;

#[test]
fn os_release_requires_a_bounded_identifier() {
    assert_eq!(
        parse_os_release_distribution("ID=debian\nVERSION_ID=13\n"),
        Some((
            crate::app::setup_attention::catalog::LinuxDistributionV1::Debian,
            Some(
                crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                    major: 13,
                    minor: 0,
                }
            ),
        ))
    );
    assert_eq!(
        parse_os_release_distribution("ID=\"fedora\"\nVERSION_ID=\"42.1\"\n"),
        Some((
            crate::app::setup_attention::catalog::LinuxDistributionV1::Fedora,
            Some(
                crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                    major: 42,
                    minor: 1,
                }
            ),
        ))
    );
    assert_eq!(
        parse_os_release_distribution("NAME=Linux\nVERSION_ID=1\n"),
        None
    );
    assert_eq!(parse_os_release_distribution("ID=\"../private\"\n"), None);
    assert_eq!(
        parse_os_release_distribution("ID=synthetic-customer-distro\n"),
        Some((
            crate::app::setup_attention::catalog::LinuxDistributionV1::Other,
            None,
        ))
    );
}

#[test]
fn subordinate_uid_and_gid_ranges_are_matched_to_the_current_identity() {
    assert!(has_subid_range(
        "developer:100000:65536\n",
        1000,
        Some("developer")
    ));
    assert!(has_subid_range("1000:200000:65536\n", 1000, None));
    assert!(!has_subid_range(
        "other:100000:65536\n",
        1000,
        Some("developer")
    ));
    assert!(!has_subid_range(
        "developer:100000:1000\n",
        1000,
        Some("developer")
    ));
    assert!(!has_subid_range(
        "developer:999999999999999999999:65536\n",
        1000,
        Some("developer")
    ));
}
