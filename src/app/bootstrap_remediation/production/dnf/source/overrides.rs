use std::path::Path;

const OVERRIDE_DIRECTORIES: [(&str, &str); 2] = [
    ("/etc/dnf/repos.override.d", "/etc/dnf"),
    ("/usr/share/dnf5/repos.override.d", "/usr/share/dnf5"),
];
const CANDIDATE_REPOSITORIES: [&str; 3] = ["fedora", "updates", "koolade-docker-ce-stable"];

#[cfg(test)]
#[path = "overrides_tests.rs"]
mod tests;

pub(super) fn reject_candidate_repository_overrides() -> anyhow::Result<()> {
    for (directory, parent) in OVERRIDE_DIRECTORIES {
        let path = Path::new(directory);
        match std::fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        super::verify_root_directory(Path::new(parent))?;
        super::verify_root_directory(path)?;
        for entry in std::fs::read_dir(path)? {
            let file = entry?.path();
            if file.extension().is_none_or(|extension| extension != "repo") {
                continue;
            }
            let contents = super::read_required_root_file(&file)?;
            reject_candidate_repository_sections(&contents)?;
        }
    }
    Ok(())
}

pub(super) fn reject_candidate_repository_sections(contents: &[u8]) -> anyhow::Result<()> {
    let text = std::str::from_utf8(contents)?;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        let Some(id) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            continue;
        };
        let id = id.trim();
        anyhow::ensure!(!id.is_empty(), "DNF repository override has an empty ID");
        anyhow::ensure!(
            !CANDIDATE_REPOSITORIES
                .iter()
                .any(|candidate| glob_may_match(id, candidate)),
            "DNF configuration section may affect a pinned package origin"
        );
    }
    Ok(())
}

fn glob_may_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    if pattern.len() > 256 || pattern.contains(&b'[') || pattern.contains(&b']') {
        return true;
    }
    let mut matches = vec![false; value.len() + 1];
    matches[0] = true;
    for token in pattern {
        let mut next = vec![false; value.len() + 1];
        if *token == b'*' {
            next[0] = matches[0];
            for index in 1..=value.len() {
                next[index] = matches[index] || next[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                next[index] = matches[index - 1] && (*token == b'?' || *token == value[index - 1]);
            }
        }
        matches = next;
    }
    matches[value.len()]
}
