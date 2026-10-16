pub(in crate::harness) fn normalize_output(lines: &[String]) -> Option<String> {
    let text = lines.join("\n").trim().to_owned();
    (!text.is_empty()).then_some(text)
}

pub(in crate::harness) fn needs_attention(lines: &[&String]) -> bool {
    lines.iter().any(|line| {
        let line = line.to_ascii_lowercase();
        ["auth", "login", "config", "policy", "copilot access"]
            .iter()
            .any(|needle| line.contains(needle))
    })
}

pub(in crate::harness) fn stderr_tail(lines: &[String]) -> String {
    const LIMIT: usize = 16_000;
    let joined = lines.join("\n");
    if joined.chars().count() <= LIMIT {
        return joined;
    }
    joined
        .chars()
        .rev()
        .take(LIMIT)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
