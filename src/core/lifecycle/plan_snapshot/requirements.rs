use super::content::stable_requirement_id;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequirementV1 {
    pub stable_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ac_ids: Vec<String>,
}

pub(super) fn parse_requirement(
    line: String,
    _acceptance_criteria: &[String],
) -> PlanRequirementV1 {
    let (requirement, metadata) = line
        .split_once(" Rationale: ")
        .map(|(requirement, metadata)| (requirement, Some(metadata)))
        .unwrap_or((line.as_str(), None));
    let (rationale, criteria) = metadata
        .map(|metadata| {
            metadata
                .split_once(" ACs: ")
                .map(|(rationale, criteria)| (Some(rationale.trim()), Some(criteria)))
                .unwrap_or((Some(metadata.trim()), None))
        })
        .unwrap_or((None, None));
    let ac_ids = criteria
        .into_iter()
        .flat_map(|value| {
            value.split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
        })
        .map(str::to_owned)
        .collect();
    PlanRequirementV1 {
        stable_id: stable_requirement_id(requirement),
        text: requirement.trim().to_owned(),
        rationale: rationale.map(str::to_owned),
        ac_ids,
    }
}

pub(super) fn parse_acceptance_criterion(
    (index, criterion): (usize, String),
) -> Result<String, String> {
    if let Some((id, text)) = criterion.split_once(':')
        && id.trim().starts_with("AC-")
    {
        let id = id.trim();
        if !valid_acceptance_id(id) || text.trim().is_empty() {
            return Err(format!(
                "acceptance criterion {id} needs an AC-nnn ID and nonempty text"
            ));
        }
        return Ok(format!("{id}: {}", text.trim()));
    }
    Ok(format!("AC-{:03}: {}", index + 1, criterion.trim()))
}

pub(super) fn valid_acceptance_id(id: &str) -> bool {
    id.strip_prefix("AC-")
        .is_some_and(|suffix| suffix.len() == 3 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(super) fn validate_acceptance_criteria(
    schema_version: u8,
    requirements: &[PlanRequirementV1],
    acceptance_criteria: &[String],
) -> Result<(), String> {
    let ids = acceptance_criteria
        .iter()
        .filter_map(|criterion| criterion.split_once(':').map(|(id, _)| id))
        .collect::<std::collections::HashSet<_>>();
    let identified = acceptance_criteria.iter().all(|criterion| {
        criterion.split_once(':').is_some_and(|(id, text)| {
            valid_acceptance_id(id) && !text.trim().is_empty()
        })
    });
    if (schema_version == 2 && !identified)
        || (identified && ids.len() != acceptance_criteria.len())
    {
        return Err("plan acceptance criteria need unique IDs and nonempty text".into());
    }
    if requirements.iter().any(|requirement| {
        requirement.acceptance_criteria_ids.iter().any(|id| {
            !acceptance_criteria.iter().any(|criterion| {
                criterion
                    .split_once(':')
                    .is_some_and(|(criterion_id, _)| criterion_id == id)
            })
        })
    }) {
        return Err("plan requirement references an unknown acceptance criterion".into());
    }
    Ok(())
}
