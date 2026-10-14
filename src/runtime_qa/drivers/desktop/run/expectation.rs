pub(in crate::runtime_qa::drivers::desktop) fn expected_state_met(
    expected: &str,
    observed: &str,
) -> bool {
    observed.to_lowercase().contains(&expected.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::expected_state_met;

    #[test]
    fn observed_desktop_state_mismatch_is_not_treated_as_unobserved() {
        assert!(expected_state_met("window visible", "main window visible"));
        assert!(!expected_state_met(
            "saved confirmation",
            "clicked window 123 at 5,5"
        ));
    }
}
