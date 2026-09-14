//! Incrementally saved task stories with an approved specification snapshot.
use crate::core::workflow::{TaskBatch, TaskBatchRef, TaskStory, WORKFLOW_FILE, Workflow};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TaskDocument {
    pub path: String,
    pub title: String,
    pub text: String,
}

pub fn slug(text: &str) -> String {
    slug_with_limit(text, 56)
}

fn slug_with_limit(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let lower: String = c.to_lowercase().collect();
        if out.len() + lower.len() > limit {
            break;
        }
        if c.is_alphanumeric() {
            out.push_str(&lower);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "feature".into()
    } else if matches!(
        out,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        format!("{out}-feature")
    } else {
        out.to_owned()
    }
}

/// Refuse links in app-owned paths before creating directories or files.
pub fn safe_directory(repo: &Path, relative: &str) -> anyhow::Result<()> {
    let mut path = repo.to_path_buf();
    for component in Path::new(relative).components() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Invalid planning directory"
        );
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "{} must be a real directory",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&path)?,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn save_workflow(repo: &Path, workflow: &Workflow) -> anyhow::Result<()> {
    safe_directory(repo, ".planner")?;
    let target = repo.join(WORKFLOW_FILE);
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        anyhow::ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "Workflow must be a regular file"
        );
    }
    let tmp = repo.join(format!(
        ".planner/.workflow-{}-{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let result = (|| -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(serde_json::to_string_pretty(workflow)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &target)?;
        Ok(())
    })();
    if tmp.exists() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

pub fn load_workflow(repo: &Path) -> anyhow::Result<Workflow> {
    match std::fs::read_to_string(repo.join(WORKFLOW_FILE)) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Workflow::default()),
        Err(e) => Err(e.into()),
    }
}

fn task_name(_feature: &str, index: usize, story: &TaskStory) -> String {
    format!("{:03}-{}.md", index + 1, slug_with_limit(&story.title, 100))
}

fn list(out: &mut String, heading: &str, values: &[String], numbered: bool) {
    out.push_str(&format!("\n## {heading}\n\n"));
    for (i, value) in values.iter().enumerate() {
        out.push_str(&format!(
            "{} {}\n",
            if numbered {
                format!("{}.", i + 1)
            } else {
                "-".into()
            },
            value.trim()
        ));
    }
}

fn render(batch: &TaskBatch, index: usize, story: &TaskStory, names: &[String]) -> String {
    let b = &batch.brief;
    let mut out = format!(
        "# {:03} — {}\n\nFeature: {}\n\nStatus: Individually validated; see batch index for generation status.\n\n## Problem this ticket solves and why\n\n{}\n\n## Ticket goal — what changes when done\n\n{}\n\n## User story\n\n{}\n\n## Purpose\n\n{}\n\n## Specification references\n\nSource: [Approved specification](specification.md)\n\n## Implementation context\n\n{}\n",
        index + 1,
        story.title.trim(),
        b.feature_name,
        story.intent,
        story.goal,
        story.user_story,
        story.purpose,
        story.context
    );
    if let Some(id) = &batch.feature_id {
        out.push_str(&format!("\nFeature ID: {id}\n"));
    }
    out.push_str(&format!(
        "Repository: {}\n",
        if story.target_repository.is_empty() {
            "root"
        } else {
            &story.target_repository
        }
    ));
    list(
        &mut out,
        "Technical design and contracts",
        &story.technical_design,
        false,
    );
    out.push_str("\n## Approved scope mapping\n");
    for r in &story.scope_items {
        out.push_str(&format!("\n- Scope {r}: {}\n", b.in_scope[r - 1]));
    }
    for r in &story.success_criteria {
        out.push_str(&format!(
            "- Success criterion {r}: {}\n",
            b.success_criteria[r - 1]
        ));
    }
    out.push_str("\n## Dependencies\n\n");
    if story.dependencies.is_empty() {
        out.push_str("None. This task can start independently.\n");
    }
    for d in &story.dependencies {
        out.push_str(&format!(
            "- [Task {d:03}]({}) must be complete.\n",
            names[d - 1]
        ));
    }
    list(
        &mut out,
        "Affected files and components",
        &story.affected_files,
        false,
    );
    list(
        &mut out,
        "Implementation steps",
        &story.implementation_steps,
        true,
    );
    list(
        &mut out,
        "Acceptance criteria",
        &story.acceptance_criteria,
        false,
    );
    list(&mut out, "Test plan", &story.test_plan, true);
    list(
        &mut out,
        "Verification commands and expected evidence",
        &story.verification_commands,
        true,
    );
    list(
        &mut out,
        "Edge cases and failure handling",
        &story.edge_cases,
        false,
    );
    list(&mut out, "Constraints", &b.constraints, false);
    list(&mut out, "Out of scope", &b.out_of_scope, false);
    out.push_str(&format!(
        "\n## Rollout and compatibility\n\n{}\n",
        story.rollout_notes
    ));
    list(
        &mut out,
        "Definition of done",
        &story.definition_of_done,
        false,
    );
    out.push_str("\n## Instructions for the implementing model\n\nRead this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.\n");
    out
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ProgressBatch {
    run: String,
    total: usize,
    brief: crate::core::workflow::InterviewBrief,
    specification: String,
    stories: Vec<TaskStory>,
}

fn progress_batches(repo: &Path) -> Vec<(String, ProgressBatch)> {
    let mut batches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(repo.join("planning/tasks")) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let directory = format!("planning/tasks/{}", entry.file_name().to_string_lossy());
            if safe_directory(repo, &directory).is_err() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path().join(".packet-progress.json")) {
                if let Ok(batch) = serde_json::from_str(&text) {
                    batches.push((directory, batch));
                }
            }
        }
    }
    batches.sort_by_key(|(directory, _)| {
        std::fs::metadata(repo.join(directory).join("README.md"))
            .and_then(|m| m.modified())
            .ok()
    });
    batches
}

fn replace_progress_file(path: &Path, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        "Refusing linked task file"
    );
    let temporary = path.with_extension(format!(
        "{}.tmp",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::write(&temporary, text)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

/// Publish only validated stories, preserving their contents on subsequent updates.
pub fn save_progress(
    repo: &Path,
    run: &str,
    batch: &TaskBatch,
    total: usize,
) -> anyhow::Result<()> {
    safe_directory(repo, "planning/tasks")?;
    let existing = progress_batches(repo)
        .into_iter()
        .find(|(_, p)| p.run == run);
    let directory = if let Some((directory, _)) = existing {
        directory
    } else {
        let feature = slug(&batch.brief.feature_name);
        let mut directory = format!("planning/tasks/{feature}");
        let mut revision = 2;
        while repo.join(&directory).exists() {
            directory = format!("planning/tasks/{feature}-{revision:02}");
            revision += 1;
        }
        std::fs::create_dir(repo.join(&directory))?;
        directory
    };
    let names: Vec<_> = batch
        .stories
        .iter()
        .enumerate()
        .map(|(i, s)| task_name("", i, s))
        .collect();
    for (i, story) in batch.stories.iter().enumerate() {
        let path = repo.join(&directory).join(&names[i]);
        let expected = render(batch, i, story, &names);
        anyhow::ensure!(
            !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
            "Refusing linked task story"
        );
        if path.exists() {
            anyhow::ensure!(
                std::fs::read_to_string(&path)? == expected,
                "Saved task was edited: {}. Preserve the edit and review before retrying.",
                path.display()
            );
        } else {
            replace_progress_file(&path, &expected)?;
        }
    }
    replace_progress_file(
        &repo.join(&directory).join("specification.md"),
        &batch.specification,
    )?;
    if let Some(contract) = &batch.contract {
        let path = repo.join(&directory).join("contract.json");
        let encoded = serde_json::to_string_pretty(contract)?;
        if path.exists() {
            anyhow::ensure!(
                std::fs::read_to_string(&path)? == encoded,
                "Frozen batch contract changed during generation"
            );
        } else {
            replace_progress_file(&path, &encoded)?;
        }
    }
    let mut index = format!(
        "# {} — task stories\n\n**Status: In progress — {} of {total} stories saved.**\n\nEach saved story is individually validated. Batch coverage and dependencies are not yet finalized. Generation can be resumed after interruption.\n\n[Approved specification](specification.md)\n\n",
        batch.brief.feature_name,
        names.len()
    );
    for (i, story) in batch.stories.iter().enumerate() {
        index.push_str(&format!("{}. [{}]({})\n", i + 1, story.title, names[i]));
    }
    replace_progress_file(&repo.join(&directory).join("README.md"), &index)?;
    replace_progress_file(
        &repo.join(&directory).join(".packet-progress.json"),
        &serde_json::to_string_pretty(&ProgressBatch {
            run: run.into(),
            total,
            brief: batch.brief.clone(),
            specification: batch.specification.clone(),
            stories: batch.stories.clone(),
        })?,
    )?;
    Ok(())
}

/// Finalize a matching incremental batch after full validation, or stage a new
/// complete batch for callers without a generation checkpoint. Preserve revisions.
pub fn write_batch(
    repo: &Path,
    batch: &TaskBatch,
    workflow: &mut Workflow,
) -> anyhow::Result<Vec<String>> {
    safe_directory(repo, "planning/tasks")?;
    safe_directory(repo, ".planner")?;
    if let Some((directory, progress)) = progress_batches(repo).into_iter().find(|(_, p)| {
        p.total == batch.stories.len()
            && p.brief == batch.brief
            && p.specification == batch.specification
            && serde_json::to_value(&p.stories).ok() == serde_json::to_value(&batch.stories).ok()
    }) {
        save_progress(repo, &progress.run, batch, progress.total)?;
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            feature: batch.brief.feature_name.clone(),
            directory: directory.clone(),
            count: batch.stories.len(),
        });
        save_workflow(repo, &next)?;
        let index_path = repo.join(&directory).join("README.md");
        let index = std::fs::read_to_string(&index_path)?;
        replace_progress_file(
            &index_path,
            &index
                .replace(
                    &format!(
                        "Status: In progress — {} of {} stories saved.",
                        batch.stories.len(),
                        batch.stories.len()
                    ),
                    "Status: Complete — all stories and batch checks validated.",
                )
                .replace(
                    "Batch coverage and dependencies are not yet finalized.",
                    "Batch coverage and dependencies are validated.",
                ),
        )?;
        std::fs::remove_file(repo.join(&directory).join(".packet-progress.json"))?;
        *workflow = next;
        let mut paths: Vec<_> = batch
            .stories
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{directory}/{}", task_name("", i, s)))
            .collect();
        paths.extend([
            format!("{directory}/README.md"),
            format!("{directory}/specification.md"),
            WORKFLOW_FILE.into(),
        ]);
        if batch.contract.is_some() {
            paths.push(format!("{directory}/contract.json"));
        }
        return Ok(paths);
    }

    let feature = slug(&batch.brief.feature_name);
    let mut directory = format!("planning/tasks/{feature}");
    let mut revision = 2;
    while std::fs::symlink_metadata(repo.join(&directory)).is_ok() {
        directory = format!("planning/tasks/{feature}-{revision:02}");
        revision += 1;
    }
    let stage = repo.join(format!(
        "planning/tasks/.packet-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir(&stage)?;
    let names: Vec<_> = batch
        .stories
        .iter()
        .enumerate()
        .map(|(i, s)| task_name(&feature, i, s))
        .collect();
    let result = (|| -> anyhow::Result<Vec<String>> {
        let mut index = format!(
            "# {} — task stories\n\nGenerated after user approval of the interview and specification.\n\n## Goal\n\n{}\n\n## Intended users\n\n{}\n\n## Intended outcome\n\n{}\n\n[Approved specification](specification.md)\n\n## Implementation order\n\n",
            batch.brief.feature_name,
            batch.brief.goal,
            batch.brief.target_users,
            batch.brief.intended_outcome
        );
        for (i, story) in batch.stories.iter().enumerate() {
            std::fs::write(stage.join(&names[i]), render(batch, i, story, &names))?;
            index.push_str(&format!("{}. [{}]({})\n", i + 1, story.title, names[i]));
        }
        list(&mut index, "Approved scope", &batch.brief.in_scope, true);
        list(
            &mut index,
            "Success criteria",
            &batch.brief.success_criteria,
            true,
        );
        std::fs::write(stage.join("README.md"), index)?;
        std::fs::write(stage.join("specification.md"), &batch.specification)?;
        if let Some(contract) = &batch.contract {
            std::fs::write(
                stage.join("contract.json"),
                serde_json::to_string_pretty(contract)?,
            )?;
        }
        anyhow::ensure!(
            !repo.join(&directory).exists(),
            "Task destination changed during generation"
        );
        std::fs::rename(&stage, repo.join(&directory))?;
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            feature: batch.brief.feature_name.clone(),
            directory: directory.clone(),
            count: names.len(),
        });
        if let Err(e) = save_workflow(repo, &next) {
            std::fs::remove_dir_all(repo.join(&directory))?;
            return Err(e);
        }
        *workflow = next;
        let mut paths: Vec<String> = names
            .iter()
            .chain(["README.md".to_owned(), "specification.md".to_owned()].iter())
            .map(|n| format!("{directory}/{n}"))
            .collect();
        paths.push(WORKFLOW_FILE.into());
        if batch.contract.is_some() {
            paths.push(format!("{directory}/contract.json"));
        }
        Ok(paths)
    })();
    if stage.exists() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}

pub fn load_latest(repo: &Path, workflow: &Workflow) -> Vec<TaskDocument> {
    let completed_time = workflow
        .task_batches
        .last()
        .and_then(|b| std::fs::metadata(repo.join(&b.directory).join("README.md")).ok())
        .and_then(|m| m.modified().ok());
    let pending = progress_batches(repo)
        .into_iter()
        .filter(|(directory, _)| {
            !workflow
                .task_batches
                .iter()
                .any(|b| &b.directory == directory)
                && std::fs::metadata(repo.join(directory).join("README.md"))
                    .and_then(|m| m.modified())
                    .ok()
                    > completed_time
        })
        .last();
    let pending_ref = pending.as_ref().map(|(directory, p)| TaskBatchRef {
        feature: p.brief.feature_name.clone(),
        directory: directory.clone(),
        count: p.stories.len(),
    });
    let Some(batch) = pending_ref
        .as_ref()
        .or_else(|| workflow.task_batches.last())
    else {
        return Vec::new();
    };
    // Only app-generated directory names may be read from the metadata.
    let Some(name) = batch.directory.strip_prefix("planning/tasks/") else {
        return Vec::new();
    };
    if name.is_empty() || name.len() > 120 || !name.chars().all(|c| c.is_alphanumeric() || c == '-')
    {
        return Vec::new();
    }
    let Ok(canonical_repo) = repo.canonicalize() else {
        return Vec::new();
    };
    let Ok(canonical_dir) = repo.join(&batch.directory).canonicalize() else {
        return Vec::new();
    };
    if !canonical_dir.starts_with(&canonical_repo) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(canonical_dir) else {
        return Vec::new();
    };
    let mut docs = Vec::new();
    for e in entries.flatten() {
        let filename = e.file_name().to_string_lossy().into_owned();
        if !filename.ends_with(".md")
            || !filename.starts_with(|c: char| c.is_ascii_digit())
            || !e.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(e.path()) {
            let title = text
                .lines()
                .next()
                .unwrap_or(&filename)
                .trim_start_matches("# ")
                .to_owned();
            docs.push(TaskDocument {
                path: format!("{}/{filename}", batch.directory),
                title,
                text,
            });
        }
    }
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    if pending.is_some() {
        if let Ok(text) = std::fs::read_to_string(repo.join(&batch.directory).join("README.md")) {
            docs.insert(
                0,
                TaskDocument {
                    path: format!("{}/README.md", batch.directory),
                    title: format!("In progress — {} stories saved", batch.count),
                    text,
                },
            );
        }
    }
    docs
}
