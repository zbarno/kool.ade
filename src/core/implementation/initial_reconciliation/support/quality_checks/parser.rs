mod commands;
mod headings;

use std::path::{Component, Path, PathBuf};

#[cfg(test)]
fn required_commands_in_markdown(
    markdown: &str,
    instruction_directory: &Path,
) -> anyhow::Result<Vec<String>> {
    required_commands_in_markdown_for_changes(markdown, instruction_directory, &[])
}

pub(in crate::core::implementation::initial_reconciliation::support) fn required_commands_in_markdown_for_changes(
    markdown: &str,
    instruction_directory: &Path,
    changed_paths: &[PathBuf],
) -> anyhow::Result<Vec<String>> {
    let mut checks = Vec::new();
    let mut quality_heading = false;
    let mut fence_is_quality = false;
    let mut in_fence = false;
    let mut required_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').to_ascii_lowercase();
            quality_heading = headings::is_quality_heading(&heading);
        }
        let applies_to_changes =
            headings::scope_applies_to_changes(trimmed, instruction_directory, changed_paths);
        if line_marks_required_checks(trimmed) && applies_to_changes {
            required_fence = true;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !in_fence {
                fence_is_quality = quality_heading || required_fence;
                required_fence = false;
                in_fence = true;
            } else {
                in_fence = false;
            }
            continue;
        }
        if in_fence {
            if fence_is_quality
                && applies_to_changes
                && let Some(command) = commands::command_line(trimmed)
            {
                checks.push(scope_command(&command, instruction_directory)?);
            }
            continue;
        }
        if applies_to_changes && (quality_heading || line_marks_required_checks(trimmed)) {
            for command in commands::inline_commands(trimmed) {
                let directory = working_directory(instruction_directory, trimmed)?;
                checks.push(scope_command(&command, &directory)?);
            }
            if trimmed.starts_with('$')
                && let Some(command) = commands::command_line(trimmed)
            {
                let directory = working_directory(instruction_directory, trimmed)?;
                checks.push(scope_command(&command, &directory)?);
            }
        }
    }
    Ok(checks)
}

fn scope_command(command: &str, directory: &Path) -> anyhow::Result<String> {
    let (directory, command) = leading_cd(command, directory)?;
    let directory = normalize_directory(&directory)?;
    if directory.as_os_str().is_empty() {
        return Ok(command.to_owned());
    }
    let directory = directory
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Validation directory is not UTF-8"))?;
    let quoted = format!("'{}'", directory.replace('\'', "'\\''"));
    Ok(format!("cd -- {quoted} && {command}"))
}

fn working_directory(instruction_directory: &Path, line: &str) -> anyhow::Result<PathBuf> {
    let mut directory = instruction_directory.to_path_buf();
    if let Some(explicit) = explicit_working_directory(line) {
        if matches!(
            explicit.as_str(),
            "." | "repo" | "repository" | "project" | "workspace"
        ) {
            directory.clear();
        } else {
            directory.push(explicit);
        }
    }
    normalize_directory(&directory)
}

fn leading_cd<'a>(command: &'a str, directory: &Path) -> anyhow::Result<(PathBuf, &'a str)> {
    let command = command.trim();
    let first = command.split_whitespace().next().unwrap_or_default();
    if first != "cd" {
        return Ok((directory.to_path_buf(), command));
    }
    let (change_directory, body) = command.split_once(" && ").ok_or_else(|| {
        anyhow::anyhow!("Validation command starts with cd but has no `&&` command")
    })?;
    let argument = change_directory
        .strip_prefix("cd ")
        .ok_or_else(|| anyhow::anyhow!("Cannot safely parse validation command directory"))?
        .trim();
    let argument = argument.strip_prefix("-- ").unwrap_or(argument).trim();
    let path = unquote_path(argument)?;
    let path = Path::new(path);
    let mut directory = directory.to_path_buf();
    directory.push(path);
    let body = body.trim();
    anyhow::ensure!(
        !body.is_empty(),
        "Validation command has no command after its directory change"
    );
    anyhow::ensure!(
        !body.split(['&', '|', ';']).any(|part| {
            part.split_whitespace()
                .next()
                .is_some_and(|token| token == "cd")
        }),
        "Validation command contains a second directory change that cannot be scoped safely"
    );
    Ok((directory, body))
}

fn unquote_path(argument: &str) -> anyhow::Result<&str> {
    let path = if (argument.starts_with('\'') && argument.ends_with('\''))
        || (argument.starts_with('"') && argument.ends_with('"'))
    {
        &argument[1..argument.len() - 1]
    } else {
        argument
    };
    anyhow::ensure!(
        !path.is_empty()
            && !path.chars().any(|character| {
                character.is_whitespace()
                    || matches!(character, '\'' | '"' | '\\' | '$' | '`' | ';' | '&' | '|')
            }),
        "Validation command directory must be a simple relative path"
    );
    Ok(path)
}

fn normalize_directory(directory: &Path) -> anyhow::Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in directory.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("Repository validation directory escapes its task repository")
            }
        }
    }
    Ok(safe)
}

fn explicit_working_directory(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let (_, suffix) = lower.rsplit_once(" from ")?;
    let start = line.len().checked_sub(suffix.len())?;
    let suffix = line[start..].trim();
    let quoted = suffix.starts_with('`');
    let suffix_lower = suffix.to_ascii_lowercase();
    if [
        "repo root",
        "repository root",
        "project root",
        "workspace root",
        "the repo root",
        "the repository root",
        "the project root",
        "the workspace root",
    ]
    .iter()
    .any(|root| suffix_lower.starts_with(root))
    {
        return Some(".".into());
    }
    let candidate = suffix
        .split_whitespace()
        .next()?
        .trim_matches(['`', '*', '_', '\'', '"'])
        .trim_end_matches([',', '.', ':', ';']);
    if candidate.ends_with('/') || candidate.contains('/') || quoted {
        Some(candidate.trim_end_matches('/').to_owned())
    } else {
        None
    }
}

pub(in crate::core::implementation::initial_reconciliation) fn report_check_is_covered(
    required: &[String],
    reported: &str,
) -> bool {
    required_command_for_report(required, reported).is_some()
}

pub(in crate::core::implementation::initial_reconciliation) fn required_command_for_report<'a>(
    required: &'a [String],
    reported: &str,
) -> Option<&'a str> {
    if required.iter().any(|command| command == reported.trim()) {
        return required
            .iter()
            .find(|command| command == &reported.trim())
            .map(String::as_str);
    }
    // Bare report entries often omit a nested AGENTS.md cwd. Bind them to a
    // unique required body, while explicit cwd changes must still match.
    let (reported_directory, reported_body) = command_context(reported)?;
    let matching_bodies = required
        .iter()
        .filter_map(|command| command_context(command))
        .filter(|(_, body)| *body == reported_body)
        .collect::<Vec<_>>();
    let matching_commands = matching_bodies
        .into_iter()
        .filter(|(directory, _)| {
            reported_directory.as_os_str().is_empty() || *directory == reported_directory
        })
        .collect::<Vec<_>>();
    if matching_commands.len() == 1 {
        let target_body = matching_commands[0].1;
        return required.iter().find_map(|command| {
            command_context(command)
                .filter(|(directory, body)| {
                    *body == target_body
                        && (reported_directory.as_os_str().is_empty()
                            || *directory == reported_directory)
                })
                .map(|_| command.as_str())
        });
    }
    None
}

fn command_context(command: &str) -> Option<(PathBuf, &str)> {
    let (directory, body) = leading_cd(command, Path::new("")).ok()?;
    let directory = normalize_directory(&directory).ok()?;
    Some((directory, body.trim()))
}

fn line_marks_required_checks(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("quality gate")
        || line.contains("required check")
        || line.contains("required test")
        || line.contains("required after")
        || line.contains("must run")
        || line.contains("before completing")
        || line.contains("before marking")
        || line.contains("before committing")
}

#[cfg(test)]
mod tests;
