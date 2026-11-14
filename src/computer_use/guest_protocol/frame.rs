use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

use super::GuestProtocolErrorV1;

pub const MAX_CONTROL_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SCREENSHOT_PNG_BYTES: usize = 2_500_000;

pub(crate) fn write_frame<T: Serialize>(
    writer: &mut (impl Write + ?Sized),
    value: &T,
) -> Result<(), GuestProtocolErrorV1> {
    let bytes = serde_json::to_vec(value).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if bytes.is_empty() || bytes.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|()| writer.write_all(&bytes))
        .and_then(|()| writer.flush())
        .map_err(|_| GuestProtocolErrorV1::Io)
}

pub(crate) fn read_frame<T: DeserializeOwned>(
    reader: &mut (impl Read + ?Sized),
) -> Result<T, GuestProtocolErrorV1> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if size == 0 || size > MAX_CONTROL_FRAME_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| GuestProtocolErrorV1::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Frame {
        value: u32,
    }

    #[test]
    fn framed_reader_rejects_partial_and_oversized_packets() {
        let mut partial = [0_u8, 0, 0, 4, b'{'].as_slice();
        assert_eq!(
            read_frame::<Frame>(&mut partial),
            Err(GuestProtocolErrorV1::Io)
        );
        let oversized_bytes = ((MAX_CONTROL_FRAME_BYTES as u32) + 1).to_be_bytes();
        let mut oversized = oversized_bytes.as_slice();
        assert_eq!(
            read_frame::<Frame>(&mut oversized),
            Err(GuestProtocolErrorV1::Oversized)
        );
    }

    #[test]
    fn framed_reader_rejects_invalid_utf8_and_unknown_fields() {
        for body in [
            b"{\"value\":\"\xff\"}".as_slice(),
            b"{\"value\":1,\"extra\":2}",
        ] {
            let mut packet = (body.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(body);
            assert_eq!(
                read_frame::<Frame>(&mut packet.as_slice()),
                Err(GuestProtocolErrorV1::Malformed)
            );
        }
    }
}
