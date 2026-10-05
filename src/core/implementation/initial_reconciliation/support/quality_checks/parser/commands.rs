pub(super) fn inline_commands(line: &str) -> Vec<String> {
    line.split('`')
        .enumerate()
        .filter_map(|(index, text)| (index % 2 == 1).then_some(text.trim()))
        .filter(|text| looks_like_command(text))
        .map(str::to_owned)
        .collect()
}

pub(super) fn command_line(line: &str) -> Option<String> {
    let line = line
        .trim_start_matches(['-', '*', ' ', '\t'])
        .trim_start_matches('$')
        .trim();
    looks_like_command(line).then(|| line.to_owned())
}

fn looks_like_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or_default();
    first == "cd"
        || matches!(
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
        )
        || (!first.starts_with('-') && first.starts_with("./"))
        || first.starts_with('/')
        || first.ends_with(".sh")
        || (!first.starts_with('-')
            && command.split_whitespace().count() > 1
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
