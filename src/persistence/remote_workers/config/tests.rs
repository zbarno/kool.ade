use super::*;

#[test]
fn runtime_trust_intersects_worker_claims_with_bundled_images_and_architecture() {
    // The bundled development lock has no promoted runtime candidates yet.
    // Model one reviewed/promoted image explicitly so the intersection logic
    // can prove it accepts that digest and rejects forged/other-platform data.
    let mut lock = crate::execution::docker::images::ImageLock::bundled().unwrap();
    let promoted = lock
        .profiles
        .get_mut("general")
        .unwrap()
        .iter_mut()
        .find(|image| image.platform == "linux/amd64")
        .unwrap();
    promoted.runtime_enabled = true;
    let local = lock
        .profiles
        .values()
        .flatten()
        .find(|image| image.runtime_enabled && image.platform == "linux/amd64")
        .unwrap()
        .digest
        .clone();
    let forged = format!("sha256:{}", "f".repeat(64));
    let selected =
        locally_approved_reported_digests(&lock, "amd64", &[local.clone(), forged.clone()]);
    assert_eq!(selected, vec![local]);
    assert!(locally_approved_reported_digests(&lock, "arm64", &[forged]).is_empty());
}

#[test]
fn unsupported_architecture_cannot_gain_a_runtime_trust_grant() {
    let lock = crate::execution::docker::images::ImageLock::bundled().unwrap();
    let reported = lock
        .profiles
        .values()
        .flatten()
        .filter(|image| image.runtime_enabled)
        .map(|image| image.digest.clone())
        .collect::<Vec<_>>();
    assert!(locally_approved_reported_digests(&lock, "unknown", &reported).is_empty());
}

#[test]
fn per_worker_resource_limit_cannot_exceed_observed_capability() {
    use crate::execution::contract::ResourceBudgetV1;
    let ceiling = ResourceBudgetV1 {
        cpu_millis: 4_000,
        memory_bytes: 2 * 1024 * 1024 * 1024,
        process_count: 128,
        temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
        output_bytes: 16 * 1024 * 1024,
    };
    let allowed = ResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 64,
        temporary_disk_bytes: 2 * 1024 * 1024 * 1024,
        output_bytes: 8 * 1024 * 1024,
    };
    ensure_within_resource_ceiling(&allowed, &ceiling).unwrap();
    let mut excess = allowed;
    excess.memory_bytes = 3 * 1024 * 1024 * 1024;
    assert!(ensure_within_resource_ceiling(&excess, &ceiling).is_err());
}
