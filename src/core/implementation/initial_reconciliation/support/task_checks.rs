use super::*;
use std::collections::BTreeSet;

pub(in crate::core::implementation) fn required_task_checks_at_commit(
    repository: &Path,
    runner: &Runner,
    base_commit: &str,
) -> anyhow::Result<Vec<String>> {
    let mut changed_paths = BTreeSet::new();
    for args in [
        vec!["diff", "--no-renames", "--name-only", "-z", base_commit],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        changed_paths.extend(
            runner
                .git_nul_records(repository, &args)?
                .into_iter()
                .map(PathBuf::from),
        );
    }

    let mut instruction_paths = BTreeSet::from([PathBuf::from("AGENTS.md")]);
    for changed in &changed_paths {
        let mut directory = changed.parent().unwrap_or_else(|| Path::new(""));
        loop {
            instruction_paths.insert(directory.join("AGENTS.md"));
            if directory.as_os_str().is_empty() {
                break;
            }
            directory = directory.parent().unwrap_or_else(|| Path::new(""));
        }
    }
    anyhow::ensure!(
        instruction_paths.len() <= 256,
        "Task instruction inventory is too large; verification is preserved for review"
    );

    let mut instructions = Vec::new();
    for path in instruction_paths {
        let path_text = path.to_string_lossy();
        let records = runner.git_nul_records(
            repository,
            &[
                "--literal-pathspecs",
                "ls-tree",
                "-z",
                base_commit,
                "--",
                path_text.as_ref(),
            ],
        )?;
        let Some(record) = records.first() else {
            continue;
        };
        let Some((metadata, listed_path)) = record.split_once('\t') else {
            anyhow::bail!("Git returned a malformed task instruction inventory record");
        };
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().unwrap_or_default();
        let object_type = fields.next().unwrap_or_default();
        if !matches!(mode, "100644" | "100755")
            || object_type != "blob"
            || listed_path != path_text.as_ref()
        {
            continue;
        }
        instructions.push((
            path_text.into_owned(),
            path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
        ));
    }

    let changed = changed_paths.iter().cloned().collect::<Vec<_>>();
    let mut checks = Vec::new();
    for (path, directory) in instructions {
        let object = format!("{base_commit}:{path}");
        let size = runner.git(repository, &["cat-file", "-s", &object])?;
        let size = size.parse::<u64>()?;
        anyhow::ensure!(
            size <= 1024 * 1024,
            "Task instructions exceed the verification snapshot size limit"
        );
        let markdown = runner.git_output_bounded(repository, &["show", &object], size)?;
        checks.extend(
            super::quality_checks::required_commands_in_markdown_for_changes(
                &markdown, &directory, &changed,
            )?,
        );
    }

    let mut unique = Vec::new();
    for check in checks {
        if !unique.contains(&check) {
            unique.push(check);
        }
    }
    anyhow::ensure!(
        unique.len() <= 256,
        "Task has too many required verification checks"
    );
    Ok(unique)
}
