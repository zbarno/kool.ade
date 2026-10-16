use crate::{
    core::workflow::TurnPurpose,
    harness::{TurnEnvelope, pi_extract::extract_json_object, responses::decode_turn_object},
};

pub(in crate::core::turn) enum EnvelopeDecode {
    Env(Box<TurnEnvelope>),
    Absent,
    Malformed(String),
}

pub(in crate::core::turn) fn decode_envelope(
    final_text: &str,
    purpose: TurnPurpose,
) -> EnvelopeDecode {
    match extract_json_object(final_text) {
        None => EnvelopeDecode::Absent,
        Some(blob) => match decode_turn_object(&blob, purpose) {
            Ok(env) => EnvelopeDecode::Env(Box::new(env)),
            Err(error) => EnvelopeDecode::Malformed(error.to_string()),
        },
    }
}

pub(in crate::core::turn) fn decode_catalogued_envelope(
    final_text: &str,
    purpose: TurnPurpose,
    manifest: Option<&crate::harness::prompt_catalog::PromptManifest>,
) -> EnvelopeDecode {
    let Some(manifest) = manifest else {
        return decode_envelope(final_text, purpose);
    };
    if manifest.role_version != crate::harness::prompt_catalog::PromptRole::PlannerConversation.id()
    {
        return decode_envelope(final_text, purpose);
    }
    if crate::harness::pi_extract::extract_json_object(final_text).is_none() {
        return EnvelopeDecode::Absent;
    }
    match crate::harness::prompt_catalog::validate_output_with_manifest(
        manifest,
        final_text,
        &crate::harness::prompt_catalog::OutputValidationContext {
            turn_purpose: purpose,
            ..Default::default()
        },
    ) {
        Ok(crate::harness::prompt_catalog::RoleOutputV1::PlanningTurn(envelope)) => {
            EnvelopeDecode::Env(envelope)
        }
        Ok(_) => EnvelopeDecode::Malformed("planner returned an unexpected schema".into()),
        Err(error) => EnvelopeDecode::Malformed(error),
    }
}
