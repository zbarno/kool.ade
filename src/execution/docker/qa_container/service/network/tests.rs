use super::attachment::{attached_only_to_network, network_is_attached};
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

#[test]
fn inspected_task_service_must_be_attached_only_to_its_leased_network() {
    let leased = NetworkInspect {
        id: Some("leased-network-id".into()),
        name: Some("koolade-task-private".into()),
        ..Default::default()
    };
    let isolated = HashMap::from([("koolade-task-private".into(), EndpointSettings::default())]);
    let dual_homed = HashMap::from([
        ("koolade-task-private".into(), EndpointSettings::default()),
        ("unexpected-egress".into(), EndpointSettings::default()),
    ]);
    let missing_lease = HashMap::from([("unexpected-egress".into(), EndpointSettings::default())]);
    let no_networks = HashMap::<String, EndpointSettings>::new();

    assert!(attached_only_to_network(&isolated, &leased));
    assert!(!attached_only_to_network(&dual_homed, &leased));
    assert!(!attached_only_to_network(&missing_lease, &leased));
    assert!(!attached_only_to_network(&no_networks, &leased));
}
