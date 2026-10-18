use super::*;

fn output(chunk: String) -> ExecutionEventV1 {
    ExecutionEventV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        sequence: 1,
        timestamp_unix_ms: 1,
        event: ExecutionEventKindV1::Stdout { chunk },
    }
}

#[test]
fn output_chunks_accept_exact_limit_and_reject_one_byte_over() {
    assert!(output("x".repeat(MAX_EVENT_CHUNK_BYTES)).validate().is_ok());
    assert!(
        output("x".repeat(MAX_EVENT_CHUNK_BYTES + 1))
            .validate()
            .is_err()
    );
}
