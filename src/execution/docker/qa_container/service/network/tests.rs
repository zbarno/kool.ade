use super::network_is_attached;
use bollard::models::{EndpointSettings, NetworkInspect};
use std::collections::HashMap;

#[test]
fn inspected_container_networks_are_matched_by_name_not_id() {
    let network = NetworkInspect {
        id: Some("docker-network-id".into()),
        name: Some("koolade-qa-private".into()),
        ..Default::default()
    };
    let attached = HashMap::from([("koolade-qa-private".into(), EndpointSettings::default())]);

    assert!(network_is_attached(&attached, &network));
}
