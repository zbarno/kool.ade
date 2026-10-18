use chrono::{DateTime, Duration, Utc};
use sha2::Digest;

use crate::{
    app::session::Project,
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
    persistence::telemetry::InvocationRecord,
};

const WINDOW_DAYS: i64 = 7;
const MINIMUM_RUNS_PER_WINDOW: usize = 3;
const MINIMUM_REGRESSION_DELTA: f64 = 0.25;

pub(super) fn regression_evidence(project: &Project, workspace_uid: &str) -> Vec<EvidenceItem> {
    let (records, _) = crate::persistence::telemetry::load(&project.chat_slug);
    telemetry_regression_evidence(workspace_uid, &records, Utc::now())
        .into_iter()
        .collect()
}

fn telemetry_regression_evidence(
    workspace_uid: &str,
    records: &[InvocationRecord],
    now: DateTime<Utc>,
) -> Option<EvidenceItem> {
    let recent_start = now - Duration::days(WINDOW_DAYS);
    let baseline_start = now - Duration::days(WINDOW_DAYS * 2);
    let baseline = outcome_counts(records, baseline_start, recent_start);
    let recent = outcome_counts(records, recent_start, now);
    if baseline.total < MINIMUM_RUNS_PER_WINDOW || recent.total < MINIMUM_RUNS_PER_WINDOW {
        return None;
    }
    let baseline_rate = baseline.failures as f64 / baseline.total as f64;
    let recent_rate = recent.failures as f64 / recent.total as f64;
    if recent_rate - baseline_rate < MINIMUM_REGRESSION_DELTA {
        return None;
    }

    let week = now
        .timestamp()
        .div_euclid(Duration::days(WINDOW_DAYS).num_seconds());
    let identity = format!("{workspace_uid}\0telemetry-regression\0{week}");
    let digest = format!("{:x}", sha2::Sha256::digest(identity.as_bytes()));
    Some(EvidenceItem {
        id: format!("telemetry-regression:{}", &digest[..24]),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "application-computed local invocation telemetry trend".into(),
        content: format!(
            "completed_or_failed_invocations baseline_7d={} failures={} rate={:.0}% recent_7d={} failures={} rate={:.0}% regression_delta={:.0} percentage_points; counts combine failed and timed-out outcomes and exclude cancellations and unknown outcomes",
            baseline.total,
            baseline.failures,
            baseline_rate * 100.0,
            recent.total,
            recent.failures,
            recent_rate * 100.0,
            (recent_rate - baseline_rate) * 100.0
        ),
    })
}

#[derive(Default)]
struct Counts {
    total: usize,
    failures: usize,
}

fn outcome_counts(
    records: &[InvocationRecord],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Counts {
    let mut counts = Counts::default();
    for record in records {
        let Some(observed_at) = record.ended_at.or(record.started_at) else {
            continue;
        };
        if observed_at < start || observed_at >= end {
            continue;
        }
        match record.outcome.as_str() {
            "completed" => counts.total += 1,
            "failed" | "timed-out" => {
                counts.total += 1;
                counts.failures += 1;
            }
            _ => {}
        }
    }
    counts
}

#[cfg(test)]
#[path = "telemetry/tests.rs"]
mod tests;
