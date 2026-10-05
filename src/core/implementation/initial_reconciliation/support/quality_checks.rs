mod parser;

use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

pub(in crate::core::implementation::initial_reconciliation) use parser::report_check_is_covered;
pub(in crate::core::implementation::initial_reconciliation) use parser::required_command_for_report;
pub(super) use parser::required_commands_in_markdown_for_changes;

pub(super) fn required_baseline_checks(
    worktree: &Path,
    changed: &str,
) -> anyhow::Result<Vec<String>> {
    let mut instruction_files = BTreeSet::new();
    let root_instructions = worktree.join("AGENTS.md");
    if root_instructions.is_file() {
        instruction_files.insert(root_instructions);
    }
    for relative in changed
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(Path::new)
    {
        let Some(parent) = relative.parent() else {
            continue;
        };
        let mut directory = worktree.join(parent);
        loop {
            let instructions = directory.join("AGENTS.md");
            if instructions.is_file() {
                instruction_files.insert(instructions);
            }
            if directory == *worktree || !directory.pop() {
                break;
            }
        }
    }

    let root = worktree.canonicalize()?;
    let mut checks = Vec::new();
    for instructions in instruction_files {
        let canonical = instructions.canonicalize()?;
        anyhow::ensure!(
            canonical.starts_with(&root),
            "Repository instructions escaped the isolated reconciliation worktree"
        );
        let instruction_directory = instructions
            .strip_prefix(worktree)?
            .parent()
            .unwrap_or_else(|| Path::new(""));
        checks.extend(parser::required_commands_in_markdown_for_changes(
            &fs::read_to_string(canonical)?,
            instruction_directory,
            &changed
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
        )?);
    }
    let mut unique = Vec::new();
    for check in checks {
        if !unique.contains(&check) {
            unique.push(check);
        }
    }
    Ok(unique)
}
