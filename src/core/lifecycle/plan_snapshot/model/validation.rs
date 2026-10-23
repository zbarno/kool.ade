use super::{PlanRequirementV1, PlanSnapshotV1};

pub(super) fn parse_acceptance_criterion(
    index: usize,
    criterion: String,
) -> Result<String, String> {
    if let Some((id, text)) = criterion.split_once(':')
        && id
            .trim()
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("AC"))
    {
        let id = id.trim();
        let suffix = id.strip_prefix("AC-").unwrap_or_default();
        if suffix.len() != 3
            || !suffix.bytes().all(|byte| byte.is_ascii_digit())
            || text.trim().is_empty()
        {
            return Err(format!(
                "acceptance criterion {id} needs an AC-nnn ID and nonempty text"
            ));
        }
        return Ok(format!("{id}: {}", text.trim()));
    }
    Ok(format!("AC-{:03}: {}", index + 1, criterion.trim()))
}

pub(super) fn validate_acceptance_criteria(snapshot: &PlanSnapshotV1) -> Result<(), String> {
    let ids = snapshot
        .acceptance_criteria
        .iter()
        .filter_map(|criterion| criterion.split_once(':').map(|(id, _)| id))
        .collect::<std::collections::HashSet<_>>();
    let identified = snapshot.acceptance_criteria.iter().all(|criterion| {
        criterion.split_once(':').is_some_and(|(id, text)| {
            let suffix = id.strip_prefix("AC-");
            suffix.is_some_and(|value| {
                value.len() == 3
                    && value.bytes().all(|byte| byte.is_ascii_digit())
                    && !text.trim().is_empty()
            })
        })
    });
    if (snapshot.schema_version == 2 && !identified)
        || (identified && ids.len() != snapshot.acceptance_criteria.len())
    {
        return Err("plan acceptance criteria need unique IDs and nonempty text".into());
    }
    if snapshot
        .requirements
        .iter()
        .any(|requirement: &PlanRequirementV1| {
            requirement.acceptance_criteria_ids.iter().any(|id| {
                !snapshot.acceptance_criteria.iter().any(|criterion| {
                    criterion
                        .split_once(':')
                        .is_some_and(|(criterion_id, _)| criterion_id == id)
                })
            })
        })
    {
        return Err("plan requirement references an unknown acceptance criterion".into());
    }
    Ok(())
}
