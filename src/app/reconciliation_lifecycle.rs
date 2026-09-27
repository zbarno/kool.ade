//! Owns the post-merge reconciliation probe and model-worker lifecycle.
//! UI effects stay with PacketApp; lifecycle timing, retries, and worker
//! ownership stay here.

use std::{
    collections::HashSet,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::{core::state::PlannerState, harness::AiHarness};

struct Probe {
    join: JoinHandle<(
        PlannerState,
        anyhow::Result<Option<crate::core::reconciliation::Candidate>>,
    )>,
}

#[derive(Default)]
pub struct Lifecycle {
    controller: Option<crate::core::reconciliation::Controller>,
    attempted: HashSet<String>,
    error: Option<String>,
    cooldown_until: Option<Instant>,
    probe: Option<Probe>,
    last_probe: Option<Instant>,
}

pub enum Event {
    Started {
        feature_id: String,
    },
    Completed {
        feature_id: String,
        state: PlannerState,
        message: String,
    },
    Deferred {
        feature_id: String,
        error: String,
        state: Option<PlannerState>,
    },
    Failed {
        feature_id: String,
        error: String,
        state: Option<PlannerState>,
    },
    ProbeFailed {
        feature_id: String,
        error: String,
    },
}

impl Lifecycle {
    pub fn is_running(&self) -> bool {
        self.controller.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn advance(
        &mut self,
        state: &PlannerState,
        harness_override: &mut Option<Box<dyn AiHarness>>,
        now: Instant,
    ) -> Option<Event> {
        if let Some(controller) = &self.controller
            && let Some(result) = controller.poll()
        {
            let feature_id = controller.feature_id.clone();
            self.controller = None;
            return Some(match result {
                Ok((state, message)) => {
                    self.cooldown_until = None;
                    self.error = None;
                    Event::Completed {
                        feature_id,
                        state,
                        message,
                    }
                }
                Err(error) => {
                    let current = PlannerState::load(&state.repo_root).ok();
                    let error = error.to_string();
                    if error.starts_with(crate::core::reconciliation::DEFER_PREFIX) {
                        self.cooldown_until = Some(now + Duration::from_secs(300));
                        Event::Deferred {
                            feature_id,
                            error,
                            state: current,
                        }
                    } else {
                        self.attempted.insert(feature_id.clone());
                        self.error = Some(error.clone());
                        Event::Failed {
                            feature_id,
                            error,
                            state: current,
                        }
                    }
                }
            });
        }

        if self.controller.is_some() || self.cooldown_until.is_some_and(|until| until > now) {
            return None;
        }
        let (feature_id, _) = state.active_feature.as_ref()?;
        if self.attempted.contains(feature_id) {
            return None;
        }

        let result = if self
            .probe
            .as_ref()
            .is_some_and(|probe| probe.join.is_finished())
        {
            self.last_probe = Some(now);
            let probe = self.probe.take().expect("finished probe exists");
            let Ok((snapshot, result)) = probe.join.join() else {
                return None;
            };
            if !same_reconciliation_inputs(&snapshot, state) {
                return None;
            }
            result
        } else {
            if self.probe.is_none()
                && self
                    .last_probe
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(3))
            {
                let snapshot = state.clone();
                let worker_snapshot = snapshot.clone();
                self.probe = Some(Probe {
                    join: std::thread::spawn(move || {
                        let result = crate::core::reconciliation::candidate(&worker_snapshot);
                        (worker_snapshot, result)
                    }),
                });
            }
            return None;
        };

        match result {
            Ok(Some(candidate)) => {
                let feature_id = candidate.feature_id.clone();
                self.attempted.insert(feature_id.clone());
                let harness = harness_override
                    .take()
                    .unwrap_or_else(|| Box::new(crate::harness::PiHarness));
                self.controller = Some(crate::core::reconciliation::Controller::start(
                    state.clone(),
                    candidate,
                    harness,
                ));
                Some(Event::Started { feature_id })
            }
            Ok(None) => None,
            Err(error) => {
                self.attempted.insert(feature_id.clone());
                let error = error.to_string();
                self.error = Some(error.clone());
                Some(Event::ProbeFailed {
                    feature_id: feature_id.clone(),
                    error,
                })
            }
        }
    }
}

fn same_reconciliation_inputs(a: &PlannerState, b: &PlannerState) -> bool {
    a.repo_root == b.repo_root
        && a.active_feature == b.active_feature
        && a.workflow == b.workflow
        && a.items == b.items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> PlannerState {
        let root = std::env::temp_dir().join(format!(
            "packet-reconciliation-lifecycle-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let state = PlannerState::load(&root).unwrap();
        let _ = std::fs::remove_dir_all(root);
        state
    }

    #[test]
    fn idle_project_does_not_launch_reconciliation_work() {
        let state = state("idle");
        let mut lifecycle = Lifecycle::default();
        let mut harness = None;
        assert!(
            lifecycle
                .advance(&state, &mut harness, Instant::now())
                .is_none()
        );
        assert!(!lifecycle.is_running());
        assert!(lifecycle.probe.is_none());
    }

    #[test]
    fn deferred_reconciliation_obeys_its_retry_cooldown() {
        let mut state = state("cooldown");
        state.active_feature = Some(("CHG-001".into(), "feature".into()));
        let now = Instant::now();
        let mut lifecycle = Lifecycle {
            cooldown_until: Some(now + Duration::from_secs(300)),
            ..Default::default()
        };
        let mut harness = None;
        assert!(lifecycle.advance(&state, &mut harness, now).is_none());
        assert!(lifecycle.probe.is_none());
    }
}
