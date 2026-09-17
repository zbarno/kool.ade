//! Open-item identifier rules.
//!
//! IDs look like `CLR-012`. They are minted by the APPLICATION (never the
//! agent's imagination): the agent may request IDs on new items, and the
//! validator reconciles — allocating fresh numbers for missing IDs and
//! rejecting collisions with existing ones.

use std::collections::HashSet;

use crate::ITEM_ID_PREFIX;

/// Numeric width of the ID suffix.
const WIDTH: usize = 3;

/// Validate an item id spelling.
pub fn is_valid_id(id: &str) -> bool {
    let Some(num) = id
        .strip_prefix(ITEM_ID_PREFIX)
        .and_then(|r| r.strip_prefix('-'))
    else {
        return false;
    };
    num.len() == WIDTH && num.bytes().all(|b| b.is_ascii_digit())
}

/// Highest numeric component in a set of valid IDs (0 when none).
pub fn max_index(ids: impl IntoIterator<Item = String>) -> u32 {
    ids.into_iter()
        .filter_map(|id| index_of(&id))
        .max()
        .unwrap_or(0)
}

/// Convert `CLR-007` → `7`.
pub fn index_of(id: &str) -> Option<u32> {
    let num = id.strip_prefix(ITEM_ID_PREFIX)?.strip_prefix('-')?;
    num.parse().ok()
}

/// Render `7` → `CLR-007`.
pub fn render(index: u32) -> String {
    format!("{ITEM_ID_PREFIX}-{:0width$}", index, width = WIDTH)
}

/// Mint the next free ID given ids already claimed (existing + this turn's
/// additions). Deterministic: lowest free number at/above (max+1)… in
/// practice we append after the max, which keeps history stable.
pub fn next_free(claimed: impl IntoIterator<Item = String>) -> String {
    let set: HashSet<String> = claimed.into_iter().filter(|id| is_valid_id(id)).collect();
    let mut cand = max_index(set.iter().cloned());
    loop {
        cand += 1;
        let id = render(cand);
        if !set.contains(&id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_tight_on_format() {
        assert!(is_valid_id("CLR-001"));
        assert!(is_valid_id("CLR-999"));
        assert!(!is_valid_id("clr-001"));
        assert!(!is_valid_id("CLR-12"));
        assert!(!is_valid_id("CLR-0001"));
        assert!(!is_valid_id("CLR-X01"));
        assert!(!is_valid_id("-CLR-01"));
    }

    #[test]
    fn next_free_appends_after_max_and_skips_gaps_at_top() {
        assert_eq!(next_free(Vec::<String>::new()), "CLR-001");
        assert_eq!(next_free(["CLR-005".to_string()]), "CLR-006");
        // Gap at bottom does not get reclaimed (stable numbering).
        assert_eq!(next_free(["CLR-001".into(), "CLR-003".into()]), "CLR-004");
    }

    #[test]
    fn round_trip_render_index() {
        assert_eq!(index_of(&render(42)), Some(42));
        assert_eq!(
            max_index(["CLR-003".to_string(), "CLR-101".to_string()]),
            101
        );
    }
}
