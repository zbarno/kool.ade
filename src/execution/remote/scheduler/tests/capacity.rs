use super::*;

#[test]
fn auto_requires_fresh_available_cpu_and_memory_that_fit_the_task() {
    let now = 1_900_000_000_000;
    let budget = budget();
    for caps in [
        BackendCapabilitiesV1 {
            available_cpu_millis: Some(1_999),
            ..capabilities(0)
        },
        BackendCapabilitiesV1 {
            available_memory_bytes: Some(budget.memory_bytes - 1),
            ..capabilities(0)
        },
    ] {
        let worker = worker("worker-a", 0, now);
        let candidates = [WorkerCandidate {
            registration: &worker,
            capabilities: &caps,
        }];
        assert_eq!(
            select_target(
                &TargetPreference::Auto,
                false,
                &candidates,
                &requirements(&budget, &[]),
                now
            ),
            Err(SelectionFailure::NoCompatibleTarget)
        );
    }
}

#[test]
fn auto_prefers_the_worker_with_more_available_resources() {
    let now = 1_900_000_000_000;
    let limited = worker("worker-a", 0, now);
    let spacious = worker("worker-b", 1, now);
    let mut limited_caps = capabilities(0);
    limited_caps.available_cpu_millis = Some(2_500);
    limited_caps.available_memory_bytes = Some(2 * 1024 * 1024 * 1024);
    limited_caps.load_millis = Some(1_500);
    limited_caps.cpu_pressure_some_avg10_basis_points = Some(500);
    let mut spacious_caps = capabilities(1);
    spacious_caps.available_cpu_millis = Some(8_000);
    spacious_caps.available_memory_bytes = Some(8 * 1024 * 1024 * 1024);
    spacious_caps.load_millis = Some(1_000);
    spacious_caps.cpu_pressure_some_avg10_basis_points = Some(200);
    let candidates = [
        WorkerCandidate {
            registration: &limited,
            capabilities: &limited_caps,
        },
        WorkerCandidate {
            registration: &spacious,
            capabilities: &spacious_caps,
        },
    ];
    let budget = budget();
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements(&budget, &[]),
            now
        )
        .unwrap(),
        ExecutionTarget::remote("worker-b")
    );
}

#[test]
fn auto_prefers_lower_observed_cpu_pressure_when_capacity_is_equal() {
    let now = 1_900_000_000_000;
    let worker_a = worker("worker-a", 0, now);
    let worker_b = worker("worker-b", 0, now);
    let mut pressured = capabilities(0);
    pressured.cpu_pressure_some_avg10_basis_points = Some(800);
    let mut idle = capabilities(0);
    idle.cpu_pressure_some_avg10_basis_points = Some(100);
    let candidates = [
        WorkerCandidate {
            registration: &worker_a,
            capabilities: &pressured,
        },
        WorkerCandidate {
            registration: &worker_b,
            capabilities: &idle,
        },
    ];
    assert_eq!(
        select_target(
            &TargetPreference::Auto,
            false,
            &candidates,
            &requirements(&budget(), &[]),
            now
        ),
        Ok(ExecutionTarget::remote("worker-b"))
    );
}
