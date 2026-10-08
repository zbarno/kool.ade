mod files;
mod merge;
use files::*;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::super::plan::Plan;
use crate::core::implementation::Implementation;

struct Move {
    source: PathBuf,
    target: PathBuf,
    state: Implementation,
    merge: Option<merge::TargetMerge>,
}

pub(super) struct MovePlan {
    repository: PathBuf,
    moves: Vec<Move>,
    locks: Vec<fs::File>,
}

impl MovePlan {
    pub(super) fn build(
        repo: &Path,
        common: &Path,
        plan: &Plan,
        task_uids: &BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
        let target_root = layout.implementation_root();
        let roots = [target_root.clone(), common.join("koolade-implementations")];
        let mut moves = Vec::new();
        let mut planned_targets = BTreeMap::<PathBuf, (PathBuf, Implementation)>::new();
        let mut locks = Vec::new();

        for root in roots {
            match fs::symlink_metadata(&root) {
                Ok(metadata) => anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Implementation evidence root {} must be a real directory",
                    root.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let source = entry.path();
                let metadata = fs::symlink_metadata(&source)?;
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "Implementation evidence path {} must be a real directory",
                    source.display()
                );
                validate_tree(&source)?;
                let state_path = source.join("state.json");
                let state_metadata = match fs::symlink_metadata(&state_path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                anyhow::ensure!(
                    state_metadata.is_file() && !state_metadata.file_type().is_symlink(),
                    "Implementation record {} must be a regular file",
                    state_path.display()
                );
                let mut state =
                    crate::core::implementation::decode_state_bytes(&fs::read(&state_path)?)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "Cannot migrate implementation record {}: {error}",
                                state_path.display()
                            )
                        })?;
                let old_ticket = state.ticket.clone();
                let new_ticket = migrated_ticket(repo, plan, &old_ticket, &state.ticket_text)?;
                let mut state_changed = state.ticket != new_ticket;
                if state.task_repository_allocation_key.is_none() {
                    state.task_repository_allocation_key =
                        Some(crate::core::implementation::key_for_ticket(&old_ticket));
                    state_changed = true;
                }
                state.ticket = new_ticket.clone();
                if let Some(uid) = task_uids.get(&new_ticket) {
                    anyhow::ensure!(
                        state
                            .task_uid
                            .as_deref()
                            .is_none_or(|current| current == uid),
                        "Implementation state for {old_ticket} points to a different task identity; preserve its evidence and resolve the conflict before connecting"
                    );
                    if state.task_uid.as_deref() != Some(uid) {
                        state.task_uid = Some(uid.clone());
                        state_changed = true;
                    }
                }
                let target =
                    target_root.join(crate::core::implementation::key_for_ticket(&new_ticket));
                check_parent_chain(
                    repo,
                    target.parent().expect("implementation root has parent"),
                )?;
                if target == source && !state_changed {
                    continue;
                }
                state.detail = state.detail.replace(
                    &source.to_string_lossy().to_string(),
                    target.to_string_lossy().as_ref(),
                );
                let merge = if target != source && target.exists() {
                    Some(merge::plan(&source, &target, &state)?)
                } else {
                    None
                };
                if target != source {
                    if let Some((other_source, other_state)) = planned_targets.get(&target) {
                        anyhow::ensure!(
                            same_tree(&source, other_source, &state, other_state)?,
                            "Two implementation records map to {}; preserve both and resolve the identity conflict before connecting",
                            target.display()
                        );
                    } else {
                        planned_targets.insert(target.clone(), (source.clone(), state.clone()));
                    }
                }
                hold_run_lock(&source.join("run.lock"), &mut locks)?;
                if target != source && target.exists() {
                    hold_run_lock(&target.join("run.lock"), &mut locks)?;
                }
                validate_task(repo, plan, &new_ticket, &state.ticket_text, &old_ticket)?;
                moves.push(Move {
                    source,
                    target,
                    state,
                    merge,
                });
            }
        }
        moves.sort_by(|a, b| a.source.cmp(&b.source));
        Ok(Self {
            repository: repo.to_path_buf(),
            moves,
            locks,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    pub(super) fn apply(self) -> anyhow::Result<()> {
        let mut remove_sources = Vec::new();
        for item in &self.moves {
            if item.source == item.target {
                let state =
                    String::from_utf8(crate::core::implementation::serialize_state(&item.state)?)?;
                crate::artifacts::atomic_write(&item.target.join("state.json"), &state)?;
                continue;
            }
            if item.target.exists() {
                merge::apply(
                    &item.source,
                    &item.target,
                    &item.state,
                    item.merge
                        .as_ref()
                        .expect("existing destination was checked during preflight"),
                )?;
            } else {
                let parent = item.target.parent().expect("state record has a parent");
                fs::create_dir_all(parent)?;
                let temporary = parent.join(format!(
                    ".koolade-migration-{}.tmp",
                    item.target
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ));
                if temporary.exists() {
                    remove_safe_tree(&temporary)?;
                }
                copy_tree(&item.source, &temporary)?;
                crate::artifacts::atomic_write_bytes(
                    &temporary.join("state.json"),
                    &crate::core::implementation::serialize_state(&item.state)?,
                )?;
                let copied = crate::core::implementation::decode_state_bytes(&fs::read(
                    temporary.join("state.json"),
                )?)?;
                anyhow::ensure!(
                    same_tree(&item.source, &temporary, &item.state, &copied)?,
                    "Could not verify copied implementation evidence for {}",
                    item.state.ticket
                );
                fs::rename(&temporary, &item.target)?;
            }
            remove_sources.push(item.source.clone());
        }

        // Release the old run.lock handles before removing their directories
        // (required on platforms that do not allow deleting an open file).
        drop(self.locks);
        for source in remove_sources {
            if source.exists() {
                remove_safe_tree(&source)?;
            }
        }
        for root in [
            self.repository
                .join(crate::artifacts::layout::canonical::IMPLEMENTATION),
            self.repository
                .join(crate::artifacts::layout::canonical::ROOT),
        ] {
            let _ = remove_if_empty(&root);
        }
        Ok(())
    }
}
