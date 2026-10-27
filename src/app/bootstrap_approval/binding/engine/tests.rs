use crate::execution::docker::images::RuntimeProfile;

use super::{approved_runtime_profile, requires_rootless_readiness_receipt};

#[test]
fn shared_image_profiles_prepare_but_only_general_records_rootless_readiness() {
    for (name, expected) in [
        ("general", RuntimeProfile::General),
        ("rust", RuntimeProfile::Rust),
        ("node", RuntimeProfile::Node),
        ("python", RuntimeProfile::Python),
        ("go", RuntimeProfile::Go),
    ] {
        let profile = approved_runtime_profile(Some(name)).expect("supported image profile");
        assert_eq!(profile, expected);
        assert_eq!(
            requires_rootless_readiness_receipt(profile),
            profile == RuntimeProfile::General
        );
    }

    assert_eq!(approved_runtime_profile(None), None);
    assert_eq!(approved_runtime_profile(Some("browser_qa")), None);
}
