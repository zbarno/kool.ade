use super::executor::proposal;
use crate::app::bootstrap_repair_policy::{Host, Operation};

#[test]
fn image_proposal_uses_only_canonical_general_profile_and_architecture() {
    for architecture in ["amd64", "arm64"] {
        let item = proposal(&format!("setup:images:general:{architecture}")).unwrap();
        assert_eq!(item.host, Host::Linux);
        assert_eq!(item.operation, Operation::PrepareLockedImage);
        assert_eq!(item.image_profile.as_deref(), Some("general"));
    }
    for invalid in [
        "setup:images:general:x86_64",
        "setup:images:rust:amd64",
        "setup:images:general:amd64:extra",
        "setup:images:general:../../amd64",
    ] {
        assert!(proposal(invalid).is_err(), "accepted {invalid}");
    }
}
