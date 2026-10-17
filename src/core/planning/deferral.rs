use super::{DeferredAssumption, QuestionPriority, QuestionStatus, QuestionV1};

/// Records an explicit deferral without converting it into a product decision.
pub fn defer_question(
    question: &mut QuestionV1,
    expected_revision: u64,
    current_plan_revision: u64,
    details: DeferredAssumption,
    deferred_by: &str,
    now_ms: u64,
) -> Result<(), String> {
    question.validate()?;
    if question.revision != expected_revision || question.plan_revision != current_plan_revision {
        return Err("stale deferral: question or plan revision changed".into());
    }
    if question.status != QuestionStatus::Open {
        return Err("only open questions can be deferred".into());
    }
    if question.priority != QuestionPriority::Optional {
        return Err("only optional questions can be deferred".into());
    }
    if deferred_by.trim().is_empty() || deferred_by.len() > 256 || now_ms == 0 {
        return Err("deferral attribution and timestamp are required".into());
    }
    if question.revision == u64::MAX {
        return Err("question revision is exhausted".into());
    }
    let mut updated = question.clone();
    updated.status = QuestionStatus::Deferred;
    updated.deferred = Some(details);
    updated.deferred_by = Some(deferred_by.to_owned());
    updated.deferred_at_ms = Some(now_ms);
    updated.revision += 1;
    updated.validate()?;
    *question = updated;
    Ok(())
}

#[cfg(test)]
#[path = "deferral/tests.rs"]
mod tests;
