use crate::core::context_build::clip;

pub(super) fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|token| !token.is_empty())
}

/// Include sections defining explicitly referenced IDs, without copying the document.
pub(super) fn referenced_sections(document: &str, subject: &str) -> String {
    let ids = tokens(subject)
        .filter(|token| {
            token.split_once('-').is_some_and(|(prefix, number)| {
                !prefix.is_empty()
                    && prefix.chars().all(|c| c.is_ascii_uppercase())
                    && !number.is_empty()
                    && number.chars().all(|c| c.is_ascii_digit())
            })
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in document.lines() {
        if (line.starts_with('#') || line.starts_with("- **") || line.starts_with('|'))
            && !current.is_empty()
        {
            sections.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    sections.push(current);
    sections
        .into_iter()
        .filter(|section| {
            // A heading or table row defining the identifier is relevant; incidental
            // references elsewhere do not pull the whole project into this thread.
            section.lines().any(|line| {
                (line.starts_with('#') || line.starts_with('|') || line.starts_with("- **"))
                    && tokens(line).any(|token| ids.contains(token))
            })
        })
        .map(|section| clip(&section, 5000))
        .collect::<Vec<_>>()
        .join("\n")
}
