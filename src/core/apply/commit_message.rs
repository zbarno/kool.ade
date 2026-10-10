use crate::core::validation::NormalizedTurn;

/// Subject like `planner: add caching policy` — agent-supplied summary when
/// sensible, otherwise derived from what actually happened (§20).
pub(super) fn compose(
    nt: &NormalizedTurn,
    spec_pre_existed: bool,
    spec_written: bool,
    n_resolved: usize,
    n_added: usize,
    n_updated: usize,
) -> String {
    let phrase_src = match (
        &nt.change_summary,
        spec_pre_existed,
        spec_written,
        n_resolved,
        n_added,
        n_updated,
    ) {
        (Some(summary), _, _, _, _, _) if !summary.is_empty() => summary.clone(),
        (_, false, true, _, _, _) => "establish initial specification".to_string(),
        (_, _, _, 0, 0, 0) => "refresh working notes".to_string(),
        (_, _, _, r, a, u) => {
            let mut parts = Vec::new();
            if r > 0 {
                parts.push(format!("resolve {r} open item{}", plural(r)));
            }
            if a > 0 {
                parts.push(format!("raise {a} open item{}", plural(a)));
            }
            if u > 0 {
                parts.push(format!("adjust {u} item{}", plural(u)));
            }
            if parts.is_empty() {
                "advance specification".to_string()
            } else {
                format!("advance specification ({})", parts.join(", "))
            }
        }
    };
    let mut phrase: String = phrase_src.chars().take(80).collect();
    if let Some((i, c)) = phrase.char_indices().next()
        && c.is_ascii_uppercase()
    {
        phrase.replace_range(i..i + c.len_utf8(), &c.to_lowercase().to_string());
    }
    if phrase.is_empty() {
        phrase = "advance specification".to_string();
    }
    format!("planner: {phrase}")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
