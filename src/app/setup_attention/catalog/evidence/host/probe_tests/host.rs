use crate::app::setup_attention::catalog::{EvidenceState as State, LinuxDistributionV1};

use super::fixture::{FixtureSource, inspect};

#[test]
fn injected_ready_host_facts_cover_all_required_probes() {
    let facts = inspect(&FixtureSource::ready());
    for state in [
        facts.kernel.state,
        facts.recipe.state,
        facts.user_namespaces.state,
        facts.subuid.state,
        facts.subgid.state,
        facts.uid_mappings.state,
        facts.helpers.state,
        facts.cgroups.state,
        facts.service_manager.state,
        facts.package_manager.state,
    ] {
        assert_eq!(state, State::Ready);
    }
}

#[test]
fn malformed_distribution_version_is_failed_without_expanding_support() {
    let mut source = FixtureSource::ready();
    source.text("/etc/os-release", "ID=ubuntu\nVERSION_ID=twenty-four\n");
    let facts = inspect(&source);
    assert_eq!(facts.recipe.state, State::Failed);
    assert_eq!(
        facts.distribution,
        Some((LinuxDistributionV1::Ubuntu, None))
    );
}
