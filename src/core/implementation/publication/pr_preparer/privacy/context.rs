const NAME_CONTEXT: &[&str] = &["customer", "employee", "name", "patient"];
const NATURAL_DETERMINERS: &[&str] = &["a", "an", "her", "his", "my", "our", "the", "their"];
const NAME_PREDICATES: &[&str] = &["asked", "reported", "requested", "said"];
const GENERIC_CONTEXT_TERMS: &[&str] = &[
    "care",
    "center",
    "contact",
    "experience",
    "service",
    "support",
    "success",
];

pub(super) fn has_lowercase_name_after_context(
    words: &[&str],
    index: usize,
    value_len: usize,
) -> bool {
    index > 0
        && NATURAL_DETERMINERS
            .iter()
            .any(|determiner| words[index - 1].eq_ignore_ascii_case(determiner))
        && NAME_CONTEXT
            .iter()
            .any(|name_context| words[index].eq_ignore_ascii_case(name_context))
        && (2..=3).contains(&value_len)
        && words[index + 1..index + value_len + 1]
            .iter()
            .all(|word| word.chars().all(|character| character.is_ascii_lowercase()))
        && !words[index + 1..index + value_len + 1].iter().any(|word| {
            GENERIC_CONTEXT_TERMS
                .iter()
                .any(|term| word.eq_ignore_ascii_case(term))
        })
        && words.get(index + value_len + 1).is_some_and(|predicate| {
            NAME_PREDICATES
                .iter()
                .any(|boundary| predicate.eq_ignore_ascii_case(boundary))
        })
}
