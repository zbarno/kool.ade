use std::collections::BTreeSet;

/// Read legacy relative dependency links only from their designated section.
/// The caller additionally checks that each target is a same-batch story.
pub fn legacy_dependencies(markdown: &str) -> anyhow::Result<Vec<String>> {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut active = false;
    let mut section = String::new();
    for line in markdown.lines() {
        if line.starts_with("## ") {
            active = line.trim().eq_ignore_ascii_case("## Dependencies");
            continue;
        }
        if active {
            section.push_str(line);
            section.push('\n');
        }
    }
    let mut links = BTreeSet::new();
    for event in Parser::new(&section) {
        if let Event::Start(Tag::Link { dest_url, .. }) = event {
            let target = dest_url.to_string();
            let path = std::path::Path::new(&target);
            anyhow::ensure!(
                path.components().count() == 1 && target.ends_with(".md"),
                "unsupported legacy dependency link {target}"
            );
            links.insert(target);
        }
    }
    anyhow::ensure!(
        links.len() <= 16,
        "Legacy task has too many dependency links"
    );
    Ok(links.into_iter().collect())
}
