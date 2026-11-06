const REGISTERED: &[(&str, &str)] = &[
    (
        "self-improvement-lifecycle",
        "self-improvement-lifecycle-v1",
    ),
    (
        "provider-routing-and-role-isolation",
        "manager-provider-route-v1",
    ),
];

pub(super) fn contains(capability_id: &str, readiness_probe_id: &str) -> bool {
    REGISTERED.contains(&(capability_id, readiness_probe_id))
}
