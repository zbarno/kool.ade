pub(super) fn validate(command: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !contains_placeholder_path(command),
        "Verification command contains a placeholder path; replace it with a real path before running checks"
    );
    anyhow::ensure!(
        !starts_long_running_server(command),
        "Verification command starts a long-running server; use a finite test harness that checks readiness and shuts the process down"
    );
    Ok(())
}

fn contains_placeholder_path(command: &str) -> bool {
    let mut remainder = command;
    while let Some(start) = remainder.find('<') {
        let after_open = &remainder[start + 1..];
        let Some(end) = after_open.find('>') else {
            return false;
        };
        let candidate = &after_open[..end];
        if !candidate.is_empty()
            && candidate
                .chars()
                .any(|character| character.is_ascii_alphabetic())
            && candidate.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '/' | '.' | '_' | '-')
            })
        {
            return true;
        }
        remainder = &after_open[end + 1..];
    }
    false
}

fn starts_long_running_server(command: &str) -> bool {
    command
        .split("&&")
        .flat_map(|part| part.split("||"))
        .flat_map(|part| part.split([';', '|', '&']))
        .any(segment_starts_server)
}

fn segment_starts_server(segment: &str) -> bool {
    let tokens = segment
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(['\'', '"', '(', ')', ','])
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>();
    tokens.windows(2).any(|pair| pair == ["dotnet", "run"])
        || package_manager_starts_server(&tokens)
}

fn package_manager_starts_server(tokens: &[String]) -> bool {
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.as_str(), "npm" | "pnpm" | "yarn" | "bun") {
            continue;
        }
        let actions = &tokens[index + 1..];
        if actions
            .first()
            .is_some_and(|action| matches!(action.as_str(), "start" | "dev" | "serve" | "preview"))
            || actions.windows(2).any(|pair| {
                pair[0] == "run"
                    && matches!(pair[1].as_str(), "start" | "dev" | "serve" | "preview")
            })
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn rejects_placeholder_paths_before_shell_execution() {
        let error = validate("npm test -- --runTestsByPath <path>").unwrap_err();
        assert!(error.to_string().contains("placeholder path"));
    }

    #[test]
    fn rejects_known_server_launch_commands_but_allows_finite_checks() {
        for command in [
            "dotnet run",
            "cd -- 'Source' && dotnet run",
            "npm start",
            "npm run dev --prefix ClientApp",
            "pnpm serve",
        ] {
            assert!(validate(command).is_err(), "accepted {command}");
        }
        for command in [
            "dotnet test",
            "npm run test",
            "cargo test",
            "test -f marker",
        ] {
            assert!(validate(command).is_ok(), "rejected {command}");
        }
    }
}
