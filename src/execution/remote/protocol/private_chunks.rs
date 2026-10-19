use super::*;

pub(crate) enum WorkerInboundFrameV1 {
    Envelope(EnvelopeV1),
    PrivateFileChunk {
        envelope: EnvelopeV1,
        chunk: crate::execution::remote::private_files::PrivateFileChunkV1,
    },
}

/// Send a secret chunk as a zeroized binary tail after its metadata-only JSON
/// envelope. The ordinary RPC writer remains unable to send this message kind.
pub(crate) fn write_private_file_chunk_frame(
    mut output: impl Write,
    envelope: &EnvelopeV1,
    chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
) -> io::Result<()> {
    if envelope.kind != EnvelopeKindV1::PrivateFileChunk {
        return Err(invalid_data("private chunk frame kind is invalid"));
    }
    envelope.validate()?;
    chunk
        .validate()
        .map_err(|_| invalid_data("private file chunk is invalid"))?;
    let header = chunk
        .header()
        .map_err(|_| invalid_data("private file chunk header is invalid"))?;
    let expected_payload = serde_json::to_value(header)
        .map_err(|_| invalid_data("private file chunk header is invalid"))?;
    if envelope.payload != expected_payload {
        return Err(invalid_data("private file chunk metadata does not match"));
    }
    let encoded_header = Zeroizing::new(
        serde_json::to_vec(envelope)
            .map_err(|_| invalid_data("private file chunk header could not be encoded"))?,
    );
    if encoded_header.is_empty() || encoded_header.len() > MAX_FRAME_BYTES {
        return Err(invalid_data(
            "private file chunk header exceeds the size limit",
        ));
    }
    let header_len = u32::try_from(encoded_header.len())
        .map_err(|_| invalid_data("private file chunk header length is invalid"))?;
    let byte_len = u32::try_from(chunk.bytes.len())
        .map_err(|_| invalid_data("private file chunk length is invalid"))?;
    output.write_all(&header_len.to_be_bytes())?;
    output.write_all(&encoded_header)?;
    output.write_all(&byte_len.to_be_bytes())?;
    output.write_all(&chunk.bytes)?;
    output.flush()
}

pub(crate) fn read_worker_frame(input: &mut impl Read) -> io::Result<Option<WorkerInboundFrameV1>> {
    let mut prefix = [0_u8; 4];
    let mut read = 0;
    while read < prefix.len() {
        match input.read(&mut prefix[read..])? {
            0 if read == 0 => return Ok(None),
            0 => return Err(invalid_data("remote worker frame prefix is truncated")),
            count => read += count,
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid_data("remote worker frame exceeds the size limit"));
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    input.read_exact(&mut bytes)?;
    let raw: RawEnvelopeV1<'_> = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_data("remote worker frame contains invalid JSON"))?;
    let private_chunk = raw.kind == EnvelopeKindV1::PrivateFileChunk;
    let (payload, chunk) = if private_chunk {
        let header: crate::execution::remote::private_files::PrivateFileChunkHeaderV1 =
            serde_json::from_str(raw.payload.get())
                .map_err(|_| invalid_data("private file chunk header is invalid"))?;
        header
            .validate()
            .map_err(|_| invalid_data("private file chunk header is invalid"))?;
        let mut chunk_prefix = [0_u8; 4];
        input.read_exact(&mut chunk_prefix)?;
        let byte_count = u32::from_be_bytes(chunk_prefix) as usize;
        if byte_count != header.byte_count as usize
            || byte_count == 0
            || byte_count > crate::execution::remote::private_files::MAX_PRIVATE_FILE_CHUNK_BYTES
        {
            return Err(invalid_data("private file chunk length is invalid"));
        }
        let mut chunk_bytes = Zeroizing::new(vec![0; byte_count]);
        input.read_exact(&mut chunk_bytes)?;
        let chunk = header
            .into_chunk(chunk_bytes)
            .map_err(|_| invalid_data("private file chunk digest is invalid"))?;
        let payload = serde_json::to_value(
            chunk
                .header()
                .map_err(|_| invalid_data("private file chunk header is invalid"))?,
        )
        .map_err(|_| invalid_data("private file chunk header is invalid"))?;
        (payload, Some(chunk))
    } else {
        let payload = serde_json::from_str(raw.payload.get())
            .map_err(|_| invalid_data("remote worker frame contains invalid JSON"))?;
        (payload, None)
    };
    let envelope = EnvelopeV1 {
        version: raw.version,
        request_id: raw.request_id,
        execution_id: raw.execution_id,
        sequence: raw.sequence,
        kind: raw.kind,
        payload,
    };
    envelope.validate()?;
    match chunk {
        Some(chunk) => Ok(Some(WorkerInboundFrameV1::PrivateFileChunk {
            envelope,
            chunk,
        })),
        None => Ok(Some(WorkerInboundFrameV1::Envelope(envelope))),
    }
}
