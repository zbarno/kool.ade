pub(super) fn validate_routing(
    routes: &std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
) -> anyhow::Result<()> {
    for (category, route) in routes {
        anyhow::ensure!(
            matches!(
                category.as_str(),
                crate::persistence::harness_settings::IMPLEMENTATION
                    | crate::persistence::harness_settings::QA
                    | crate::persistence::harness_settings::DOCUMENTATION
            ),
            "Task routing cannot override category '{category}'"
        );
        anyhow::ensure!(
            !route.harness.trim().is_empty()
                && route.harness.len() <= 128
                && route
                    .harness
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "-_".contains(ch)),
            "Task routing has an invalid harness ID"
        );
        if let Some(model) = &route.model {
            anyhow::ensure!(
                !model.trim().is_empty()
                    && model.len() <= 512
                    && !model.chars().any(char::is_control),
                "Task routing has an invalid model ID"
            );
        }
    }
    Ok(())
}
