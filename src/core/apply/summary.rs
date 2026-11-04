use crate::core::validation::NormalizedTurn;

/// Describe the local save without implying that a draft was published.
pub(super) fn compose(
    nt: &NormalizedTurn,
    draft_written: bool,
    n_resolved: usize,
    n_added: usize,
    n_updated: usize,
) -> String {
    let phrase_src = match (
        &nt.change_summary,
        draft_written,
        n_resolved,
        n_added,
        n_updated,
    ) {
        (Some(summary), _, _, _, _) if !summary.is_empty() => summary.clone(),
        (_, true, _, _, _) => "save private specification proposal".to_string(),
        (_, _, 0, 0, 0) => "refresh working notes".to_string(),
        (_, _, r, a, u) => {
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
                "save local planning state".to_string()
            } else {
                format!("save local planning state ({})", parts.join(", "))
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
        phrase = "save local planning state".to_string();
    }
    phrase
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
