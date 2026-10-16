#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DependencyGroup {
    pub alternatives: Vec<DependencyAlternative>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DependencyAlternative {
    pub name: String,
    pub qualifier: Option<String>,
    pub constraint: Option<VersionConstraint>,
    arch_restrictions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VersionConstraint {
    operator: VersionOperator,
    version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionOperator {
    Less,
    LessEqual,
    Equal,
    GreaterEqual,
    Greater,
}

pub(super) fn parse_dependencies(value: &str) -> anyhow::Result<Vec<DependencyGroup>> {
    split_top_level(value, ',')?
        .into_iter()
        .map(|group| {
            let alternatives = split_top_level(group, '|')?
                .into_iter()
                .map(parse_alternative)
                .collect::<anyhow::Result<Vec<_>>>()?;
            anyhow::ensure!(!alternatives.is_empty(), "empty Debian dependency group");
            Ok(DependencyGroup { alternatives })
        })
        .collect()
}

fn split_top_level(value: &str, separator: char) -> anyhow::Result<Vec<&str>> {
    let (mut parens, mut brackets) = (0_u8, 0_u8);
    let mut start = 0;
    let mut output = Vec::new();
    for (index, character) in value.char_indices() {
        match character {
            '(' => {
                parens = parens
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("dependency syntax overflow"))?
            }
            ')' => {
                parens = parens
                    .checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("unbalanced dependency parentheses"))?
            }
            '[' => {
                brackets = brackets
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("dependency syntax overflow"))?
            }
            ']' => {
                brackets = brackets
                    .checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("unbalanced architecture brackets"))?
            }
            c if c == separator && parens == 0 && brackets == 0 => {
                output.push(value[start..index].trim());
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    anyhow::ensure!(
        parens == 0 && brackets == 0,
        "unbalanced Debian dependency expression"
    );
    output.push(value[start..].trim());
    anyhow::ensure!(
        output.iter().all(|part| !part.is_empty()),
        "empty Debian dependency expression"
    );
    Ok(output)
}

fn parse_alternative(value: &str) -> anyhow::Result<DependencyAlternative> {
    let token_end = value
        .find(char::is_whitespace)
        .unwrap_or(value.len())
        .min(value.find('(').unwrap_or(value.len()))
        .min(value.find('[').unwrap_or(value.len()));
    let token = &value[..token_end];
    let (name, qualifier) = token
        .split_once(':')
        .map_or((token, None), |(name, qualifier)| (name, Some(qualifier)));
    anyhow::ensure!(valid_name(name), "invalid Debian dependency package name");
    anyhow::ensure!(
        qualifier.is_none_or(valid_architecture_qualifier),
        "unsupported Debian package architecture qualifier"
    );
    let mut rest = value[token_end..].trim();
    let mut constraint = None;
    let mut arch_restrictions = Vec::new();
    loop {
        if rest.starts_with('(') {
            anyhow::ensure!(constraint.is_none(), "duplicate Debian version constraint");
            let end = rest
                .find(')')
                .ok_or_else(|| anyhow::anyhow!("unterminated Debian version constraint"))?;
            let (operator, version) = parse_constraint(rest[1..end].trim())?;
            constraint = Some(VersionConstraint { operator, version });
            rest = rest[end + 1..].trim();
        } else if rest.starts_with('[') {
            anyhow::ensure!(
                arch_restrictions.is_empty(),
                "duplicate Debian architecture restriction"
            );
            let end = rest
                .find(']')
                .ok_or_else(|| anyhow::anyhow!("unterminated architecture restriction"))?;
            arch_restrictions = rest[1..end]
                .split_ascii_whitespace()
                .map(str::to_owned)
                .collect();
            anyhow::ensure!(
                !arch_restrictions.is_empty()
                    && arch_restrictions.iter().all(|arch| arch
                        .strip_prefix('!')
                        .unwrap_or(arch)
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')),
                "invalid Debian architecture restriction"
            );
            rest = rest[end + 1..].trim();
        } else {
            break;
        }
    }
    anyhow::ensure!(rest.is_empty(), "unsupported Debian dependency syntax");
    Ok(DependencyAlternative {
        name: name.into(),
        qualifier: qualifier.map(str::to_owned),
        constraint,
        arch_restrictions,
    })
}

fn parse_constraint(value: &str) -> anyhow::Result<(VersionOperator, String)> {
    let (operator, version) = [
        (">=", VersionOperator::GreaterEqual),
        ("<=", VersionOperator::LessEqual),
        (">>", VersionOperator::Greater),
        ("<<", VersionOperator::Less),
        ("=", VersionOperator::Equal),
    ]
    .into_iter()
    .find_map(|(prefix, operator)| {
        value
            .strip_prefix(prefix)
            .map(|version| (operator, version.trim()))
    })
    .ok_or_else(|| anyhow::anyhow!("unsupported Debian version operator"))?;
    validate_version(version)?;
    Ok((operator, version.to_owned()))
}

pub(super) fn parse_provided_version(value: &str) -> anyhow::Result<(String, String)> {
    let (operator, version) = [
        ("=", "="),
        (">=", ">="),
        ("<=", "<="),
        (">>", ">>"),
        ("<<", "<<"),
    ]
    .into_iter()
    .find_map(|(prefix, operator)| {
        value
            .strip_prefix(prefix)
            .map(|version| (operator, version.trim()))
    })
    .ok_or_else(|| anyhow::anyhow!("unsupported Debian Provides version operator"))?;
    validate_version(version)?;
    Ok((operator.into(), version.into()))
}

pub(super) fn matches_target_arch(alternative: &DependencyAlternative, arch: &str) -> bool {
    let positives = alternative
        .arch_restrictions
        .iter()
        .filter(|item| !item.starts_with('!'))
        .collect::<Vec<_>>();
    let matches = |pattern: &str| match pattern.split_once('-') {
        Some(("linux", suffix)) => suffix == "any" || suffix == arch,
        Some(("any", suffix)) => suffix == "any" || suffix == arch,
        _ => pattern == arch,
    };
    let positive_match = positives.is_empty() || positives.iter().any(|item| matches(item));
    positive_match
        && !alternative
            .arch_restrictions
            .iter()
            .filter_map(|item| item.strip_prefix('!'))
            .any(matches)
}

pub(super) fn satisfies_version(actual: &str, constraint: Option<&VersionConstraint>) -> bool {
    let Some(constraint) = constraint else {
        return true;
    };
    let Ok(ordering) = debian_version_cmp(actual, &constraint.version) else {
        return false;
    };
    match constraint.operator {
        VersionOperator::Less => ordering.is_lt(),
        VersionOperator::LessEqual => !ordering.is_gt(),
        VersionOperator::Equal => ordering.is_eq(),
        VersionOperator::GreaterEqual => !ordering.is_lt(),
        VersionOperator::Greater => ordering.is_gt(),
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
}

fn valid_architecture_qualifier(value: &str) -> bool {
    matches!(value, "any" | "native")
        || (!value.is_empty()
            && value != "foreign"
            && value.len() <= 32
            && value.as_bytes()[0].is_ascii_lowercase()
            && value
                .as_bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versioned_alternatives_and_architecture_filters() {
        let parsed =
            parse_dependencies("libc6 (>= 2.34) | libc6-udeb (<< 1.0), libssl3:any [amd64 !arm64]")
                .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].alternatives.len(), 2);
        assert!(matches_target_arch(&parsed[1].alternatives[0], "amd64"));
        assert!(!matches_target_arch(&parsed[1].alternatives[0], "arm64"));
        assert!(parse_dependencies("libssl3 (>= 1.0").is_err());
        assert!(parse_dependencies("libssl3 <!nocheck>").is_err());
    }

    #[test]
    fn debian_version_ordering_matches_epoch_tilde_and_revision_rules() {
        assert!(debian_version_cmp("2:1.0", "1:99.0").unwrap().is_gt());
        assert!(debian_version_cmp("1.0~rc1", "1.0").unwrap().is_lt());
        assert!(debian_version_cmp("1.0-2", "1.0-1").unwrap().is_gt());
        assert!(debian_version_cmp("1.0002", "1.2").unwrap().is_eq());
        assert!(satisfies_version(
            "1.2-1",
            Some(&VersionConstraint {
                operator: VersionOperator::GreaterEqual,
                version: "1.1".into(),
            })
        ));
    }
}
mod version;
use version::{compare as debian_version_cmp, validate_version};
