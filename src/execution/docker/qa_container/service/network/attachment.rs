use bollard::models::NetworkInspect;
use std::collections::HashMap;

pub(super) fn network_is_attached<T>(
    networks: &HashMap<String, T>,
    network: &NetworkInspect,
) -> bool {
    network
        .name
        .as_deref()
        .is_some_and(|name| networks.contains_key(name))
}

pub(super) fn attached_only_to_network<T>(
    networks: &HashMap<String, T>,
    network: &NetworkInspect,
) -> bool {
    networks.len() == 1 && network_is_attached(networks, network)
}
