use crate::error::AppError;

/// Validates a requested reasoning level against the provider CLI contract and
/// returns the exact provider value. Model-specific incompatibilities remain
/// the provider's responsibility and are reported as provider failures.
pub(crate) fn validate_reasoning_level(
    provider: &str,
    requested: &str,
) -> Result<&'static str, AppError> {
    let value = requested.trim().to_ascii_lowercase();
    let supported: &[&str] = match provider {
        "pi" => &["off", "minimal", "low", "medium", "high", "xhigh"],
        "codex" => &["minimal", "low", "medium", "high", "xhigh"],
        "claude" => &["low", "medium", "high", "xhigh", "max"],
        "antigravity" => &["low", "medium", "high"],
        "opencode" => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        "copilot" => &["low", "medium", "high", "xhigh", "max"],
        _ => &[],
    };
    if let Some(value) = supported.iter().find(|candidate| **candidate == value) {
        return Ok(match (provider, *value) {
            ("opencode", "off") => "none",
            (_, value) => match value {
                "off" => "off",
                "minimal" => "minimal",
                "low" => "low",
                "medium" => "medium",
                "high" => "high",
                "xhigh" => "xhigh",
                "max" => "max",
                _ => unreachable!("supported reasoning levels are enumerated above"),
            },
        });
    }

    Err(AppError::ProviderCapabilityMismatch {
        provider: provider.to_owned(),
        missing: vec![format!(
            "reasoning level '{}' (supported by this CLI: {})",
            requested.trim(),
            if supported.is_empty() {
                "none known".to_owned()
            } else {
                supported.join(", ")
            }
        )],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_levels_are_provider_specific_and_never_silently_downgraded() {
        for (provider, level, expected) in [
            ("pi", "OFF", "off"),
            ("codex", "xhigh", "xhigh"),
            ("claude", "xhigh", "xhigh"),
            ("antigravity", "medium", "medium"),
            ("opencode", "off", "none"),
            ("copilot", "max", "max"),
        ] {
            assert_eq!(validate_reasoning_level(provider, level).unwrap(), expected);
        }

        for (provider, level) in [
            ("codex", "off"),
            ("claude", "off"),
            ("antigravity", "xhigh"),
            ("copilot", "minimal"),
            ("unknown", "medium"),
            ("pi", "unsupported"),
        ] {
            assert!(matches!(
                validate_reasoning_level(provider, level),
                Err(AppError::ProviderCapabilityMismatch { .. })
            ));
        }
    }
}
