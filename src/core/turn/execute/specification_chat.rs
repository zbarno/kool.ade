use super::{TurnInputs, TurnOutcome, revision_guard};

pub(super) fn mode_matches(inputs: &TurnInputs, read_only: bool) -> bool {
    inputs.specification_chat.as_ref().is_none_or(|request| {
        matches!(
            (request.intent, read_only),
            (crate::core::specification_chat::Intent::Ask, true)
                | (crate::core::specification_chat::Intent::Propose, false)
        ) && request.text == inputs.user_message
    })
}

pub(super) fn stale_target_problem(inputs: &TurnInputs) -> Option<String> {
    let request = inputs.specification_chat.as_ref()?;
    revision_guard::unchanged_specification_target(&inputs.state, &request.target).err()
}

pub(super) fn validate_proposal(
    inputs: &TurnInputs,
    normalized: &mut crate::core::validation::NormalizedTurn,
) -> Result<(), String> {
    let Some(request) = &inputs.specification_chat else {
        return Ok(());
    };
    crate::core::specification_chat::validate_proposal(&request.target, normalized, &inputs.state)
}

pub(super) fn discussion_text(
    inputs: &TurnInputs,
    final_text: String,
    manifest: Option<&crate::harness::prompt_catalog::PromptManifest>,
) -> String {
    if inputs.specification_chat.is_none() {
        return final_text;
    }
    match super::decode_catalogued_envelope(&final_text, inputs.purpose, manifest) {
        super::EnvelopeDecode::Env(envelope) => envelope.assistant().to_owned(),
        super::EnvelopeDecode::Absent | super::EnvelopeDecode::Malformed(_) => final_text,
    }
}

pub(super) fn rejected(
    problem: String,
    final_text: String,
    elapsed: std::time::Duration,
) -> TurnOutcome {
    TurnOutcome::Rejected {
        problems: vec![problem],
        final_text,
        elapsed,
    }
}
