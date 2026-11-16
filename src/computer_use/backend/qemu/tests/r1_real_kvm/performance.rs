use std::{fs, path::Path};

const PROVISIONING_BUDGET_MS: u128 = 180_000;
const SCREEN_OBSERVATION_BUDGET_MS: u128 = 2_000;
const IDLE_MEMORY_BUDGET_BYTES: u64 = 1_610_612_736; // 1.5 GiB
const IDLE_CPU_BUDGET_USEC_PER_MS: u64 = 2_500; // 2.5 CPU-seconds per wall second
const PEAK_MEMORY_BUDGET_BYTES: u64 = 1_879_048_192; // 1.75 GiB of the 2 GiB cgroup limit
const CLEANUP_BUDGET_MS: u128 = 10_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct AcceptanceMetrics {
    pub provisioning_ms: u128,
    pub screen_observation_ms: u128,
    pub idle_sample_window_ms: u128,
    pub idle_memory_bytes: u64,
    pub idle_cpu_usec: u64,
    pub peak_memory_bytes: u64,
    pub cleanup_ms: u128,
}

pub(super) fn assert_acceptance_budgets(metrics: AcceptanceMetrics) -> Result<(), String> {
    if metrics.idle_sample_window_ms == 0 {
        return Err("real KVM idle CPU sample window must be nonzero".into());
    }
    let idle_cpu_budget = metrics
        .idle_sample_window_ms
        .saturating_mul(u128::from(IDLE_CPU_BUDGET_USEC_PER_MS));
    if u128::from(metrics.idle_cpu_usec) > idle_cpu_budget {
        return Err(format!(
            "real KVM idle CPU exceeded regression budget: {} > {idle_cpu_budget} usec",
            metrics.idle_cpu_usec
        ));
    }
    for (name, observed, budget) in [
        (
            "provisioning",
            metrics.provisioning_ms,
            PROVISIONING_BUDGET_MS,
        ),
        (
            "screen observation",
            metrics.screen_observation_ms,
            SCREEN_OBSERVATION_BUDGET_MS,
        ),
        ("cleanup", metrics.cleanup_ms, CLEANUP_BUDGET_MS),
    ] {
        if observed > budget {
            return Err(format!(
                "real KVM {name} exceeded regression budget: {observed}ms > {budget}ms"
            ));
        }
    }
    for (name, observed, budget) in [
        (
            "idle memory",
            metrics.idle_memory_bytes,
            IDLE_MEMORY_BUDGET_BYTES,
        ),
        (
            "session peak memory",
            metrics.peak_memory_bytes,
            PEAK_MEMORY_BUDGET_BYTES,
        ),
    ] {
        if observed > budget {
            return Err(format!(
                "real KVM {name} exceeded regression budget: {observed} > {budget}"
            ));
        }
    }
    Ok(())
}

pub(super) fn read_cgroup_value(cgroup_directory: &Path, name: &str) -> Result<u64, String> {
    let path = cgroup_directory.join(name);
    fs::read_to_string(&path)
        .map_err(|error| format!("read cgroup metric {}: {error}", path.display()))?
        .trim()
        .parse()
        .map_err(|error| format!("parse cgroup metric {}: {error}", path.display()))
}

pub(super) fn read_cpu_usage_usec(cgroup_directory: &Path) -> Result<u64, String> {
    let path = cgroup_directory.join("cpu.stat");
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("read cgroup metric {}: {error}", path.display()))?;
    contents
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            (name == "usage_usec").then_some(value)
        })
        .ok_or_else(|| format!("cgroup CPU metric is unavailable: {}", path.display()))?
        .parse()
        .map_err(|error| format!("parse cgroup CPU metric {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{AcceptanceMetrics, assert_acceptance_budgets};

    fn within_budgets() -> AcceptanceMetrics {
        AcceptanceMetrics {
            provisioning_ms: 90_000,
            screen_observation_ms: 500,
            idle_sample_window_ms: 1_000,
            idle_memory_bytes: 1_200_000_000,
            idle_cpu_usec: 1_000_000,
            peak_memory_bytes: 1_500_000_000,
            cleanup_ms: 500,
        }
    }

    #[test]
    fn accepts_metrics_within_the_documented_regression_budgets() {
        assert_acceptance_budgets(within_budgets()).unwrap();
    }

    #[test]
    fn rejects_each_performance_regression_budget_independently() {
        let mut cases = Vec::new();
        let mut metrics = within_budgets();
        metrics.provisioning_ms = 180_001;
        cases.push(metrics);
        let mut metrics = within_budgets();
        metrics.screen_observation_ms = 2_001;
        cases.push(metrics);
        let mut metrics = within_budgets();
        metrics.idle_memory_bytes = 1_610_612_737;
        cases.push(metrics);
        let mut metrics = within_budgets();
        metrics.idle_cpu_usec = 2_500_001;
        cases.push(metrics);
        let mut metrics = within_budgets();
        metrics.peak_memory_bytes = 1_879_048_193;
        cases.push(metrics);
        let mut metrics = within_budgets();
        metrics.cleanup_ms = 10_001;
        cases.push(metrics);
        assert!(
            cases
                .into_iter()
                .all(|metrics| { assert_acceptance_budgets(metrics).is_err() })
        );
    }
}
