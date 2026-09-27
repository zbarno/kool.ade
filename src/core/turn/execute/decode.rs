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
