use std::{collections::BTreeMap, fs, path::Path};

use super::super::plan::Plan;
use crate::core::{implementation::Failure, implementation_queue::Queue};

pub(super) struct QueuePlan {
    path: Option<std::path::PathBuf>,
    contents: Option<Vec<u8>>,
}

impl QueuePlan {
    pub(super) fn build(repo: &Path, common: &Path, plan: &Plan) -> anyhow::Result<Self> {
        let path = common.join("packet-queue.json");
        match fs::symlink_metadata(&path) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Task queue state {} must be a regular file",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: None,
                    contents: None,
                });
            }
            Err(error) => return Err(error.into()),
        }
        let bytes = fs::read(&path)?;
        let (mut queue, legacy_state) = Queue::decode_persisted(&bytes)
            .map_err(|error| anyhow::anyhow!("Cannot migrate task queue state: {error}"))?;
        let original = serde_json::to_value(&queue)?;
        queue.current_ticket = queue
            .current_ticket
            .take()
            .map(|ticket| relocate(repo, plan, ticket))
            .transpose()?;
        queue.in_flight = std::mem::take(&mut queue.in_flight)
            .into_iter()
            .map(|ticket| relocate(repo, plan, ticket))
            .collect::<anyhow::Result<_>>()?;
        let mut blocked = BTreeMap::<String, Failure>::new();
        for (ticket, mut failure) in std::mem::take(&mut queue.blocked) {
            let current_ticket = relocate(repo, plan, ticket.clone())?;
            failure.message =
                relocate_report_references(repo, common, &ticket, &current_ticket, failure.message);
            blocked
                .entry(current_ticket)
                .and_modify(|prior| {
                    if prior.kind != failure.kind || prior.recovery != failure.recovery {
                        prior.kind = crate::core::implementation::FailureKind::Other;
                        prior.recovery =
                            crate::core::implementation::RecoveryDisposition::UserAction;
                    }
                    if prior.message != failure.message {
                        prior.message.push_str(&format!("\n{}", failure.message));
                    }
                })
                .or_insert(failure);
        }
        queue.blocked = blocked;
        let mut attempts = BTreeMap::<String, usize>::new();
        for (ticket, count) in std::mem::take(&mut queue.recovery_attempts) {
            let saved = attempts.entry(relocate(repo, plan, ticket)?).or_default();
            *saved = (*saved).max(count);
        }
        queue.recovery_attempts = attempts;
        let contents = (legacy_state || serde_json::to_value(&queue)? != original).then(|| {
            serde_json::to_vec_pretty(&queue).expect("queue serializes after successful parse")
        });
        Ok(Self {
            path: Some(path),
            contents,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.contents.is_none()
    }

    pub(super) fn apply(self) -> anyhow::Result<()> {
        let (Some(path), Some(contents)) = (self.path, self.contents) else {
            return Ok(());
        };
        crate::artifacts::atomic_write_bytes(&path, &contents)
    }
}

fn relocate_report_references(
    repo: &Path,
    common: &Path,
    old_ticket: &str,
    new_ticket: &str,
    mut detail: String,
) -> String {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    let old_key = crate::core::implementation::key_for_ticket(old_ticket);
    let new_directory = layout
        .implementation_root()
        .join(crate::core::implementation::key_for_ticket(new_ticket));
    for root in [
        common.join("packet-implementations"),
        layout.implementation_root(),
    ] {
        let old_directory = root.join(&old_key);
        detail = detail.replace(
            &old_directory.to_string_lossy().to_string(),
            new_directory.to_string_lossy().as_ref(),
        );
    }
    detail
}

fn relocate(repo: &Path, plan: &Plan, ticket: String) -> anyhow::Result<String> {
    let Some(target) = super::super::plan::relocated_ticket_path(&ticket) else {
        return Ok(ticket);
    };
    let exists = plan.planned_content(&target).is_some() || repo.join(&target).is_file();
    anyhow::ensure!(
        exists,
        "Queue state refers to missing legacy task {ticket}; preserve the queue and restore that task before connecting"
    );
    Ok(target)
}
