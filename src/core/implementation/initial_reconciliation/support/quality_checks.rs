use std::{collections::BTreeSet, fs, path::Path};

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
        checks.extend(required_commands_in_markdown(&fs::read_to_string(
            canonical,
        )?));
    }
    let mut unique = Vec::new();
    for check in checks {
        if !unique.contains(&check) {
            unique.push(check);
        }
    }
    Ok(unique)
}

pub(super) fn required_commands_in_markdown(markdown: &str) -> Vec<String> {
    let mut checks = Vec::new();
    let mut quality_heading = false;
    let mut fence_is_quality = false;
    let mut in_fence = false;
    let mut required_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').to_ascii_lowercase();
            quality_heading = [
                "quality",
                "verification",
                "test",
                "check",
                "validation",
                "gate",
            ]
            .iter()
            .any(|word| heading.contains(word));
        }
        if line_marks_required_checks(trimmed) {
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
            if fence_is_quality && let Some(command) = command_line(trimmed) {
                checks.push(command);
            }
            continue;
        }
        if quality_heading || line_marks_required_checks(trimmed) {
            checks.extend(inline_commands(trimmed));
            if trimmed.starts_with('$')
                && let Some(command) = command_line(trimmed)
            {
                checks.push(command);
            }
        }
    }
    checks
}

fn line_marks_required_checks(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("quality gate")
        || line.contains("required check")
        || line.contains("required test")
        || line.contains("must run")
        || line.contains("before completing")
        || line.contains("before marking")
        || line.contains("before committing")
}

fn inline_commands(line: &str) -> Vec<String> {
    line.split('`')
        .enumerate()
        .filter_map(|(index, text)| (index % 2 == 1).then_some(text.trim()))
        .filter(|text| looks_like_command(text))
        .map(str::to_owned)
        .collect()
}

fn command_line(line: &str) -> Option<String> {
    let line = line
        .trim_start_matches(['-', '*', ' ', '\t'])
        .trim_start_matches('$')
        .trim();
    looks_like_command(line).then(|| line.to_owned())
}

fn looks_like_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    matches!(
        first,
        "cargo"
            | "rustfmt"
            | "clippy-driver"
            | "make"
            | "just"
            | "npm"
            | "npx"
            | "pnpm"
            | "yarn"
            | "bun"
            | "pytest"
            | "python"
            | "python3"
            | "tox"
            | "go"
            | "mvn"
            | "gradle"
            | "dotnet"
            | "swift"
            | "xcodebuild"
            | "composer"
            | "php"
            | "ruby"
            | "bundle"
            | "rake"
            | "ctest"
            | "cmake"
            | "ninja"
            | "test"
            | "./gradlew"
            | "sh"
            | "bash"
            | "dash"
            | "nix"
            | "docker"
            | "podman"
            | "deno"
            | "mix"
            | "rebar3"
            | "zig"
            | "lein"
            | "clojure"
            | "git"
            | "node"
    ) || first.starts_with("./")
        || first.starts_with('/')
        || first.ends_with(".sh")
        || (command.split_whitespace().count() > 1
            && first.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_' | '.' | '/')
            })
            && !matches!(
                first,
                "run" | "before" | "after" | "then" | "ensure" | "check"
            ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_inline_custom_and_fenced_required_commands() {
        let commands = required_commands_in_markdown(
            "## Quality Gates\n\n- Run `./scripts/quality.sh --strict` and `nix develop -c cargo test`.\n\nBefore marking complete, run these too:\n```sh\nbash scripts/smoke.sh\n```\n",
        );

        assert_eq!(
            commands,
            [
                "./scripts/quality.sh --strict",
                "nix develop -c cargo test",
                "bash scripts/smoke.sh"
            ]
        );
    }

    #[test]
    fn does_not_turn_plain_quality_guidance_into_a_command() {
        let commands = required_commands_in_markdown(
            "## Quality Gates\n\n- Run every required project test before completion.\n",
        );

        assert!(commands.is_empty());
    }
}
