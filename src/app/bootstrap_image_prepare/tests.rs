use super::executor::proposal;
use crate::app::bootstrap_repair_policy::{Host, Operation};

const ENGINE_IDENTITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn image_proposal_uses_only_canonical_profiles_architectures_and_engine_identity() {
    for profile in ["general", "rust", "node", "python", "go"] {
        for architecture in ["amd64", "arm64"] {
            let task_id = format!("setup:images:{profile}:{architecture}:{ENGINE_IDENTITY}");
            let item = proposal(&task_id).unwrap();
            assert_eq!(item.host, Host::Linux);
            assert_eq!(item.operation, Operation::PrepareLockedImage);
            assert_eq!(item.image_profile.as_deref(), Some(profile));
            assert_eq!(item.prerequisite_id, task_id);
        }
    }
    for invalid in [
        "setup:images:general:amd64",
        "setup:images:browser_qa:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "setup:images:general:x86_64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag",
        "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:extra",
        "setup:images:general:../../amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(proposal(invalid).is_err(), "accepted {invalid}");
    }
}
