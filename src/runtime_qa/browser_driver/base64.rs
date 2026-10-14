pub(super) fn decode_base64(value: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if !value.len().is_multiple_of(4)
        || value.len().saturating_mul(3) / 4 > max_bytes.saturating_add(2)
    {
        return Err("browser screenshot encoding exceeds its byte bound".into());
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for block in value.as_bytes().as_chunks::<4>().0 {
        let pad = (block[3] == b'=') as usize + (block[2] == b'=') as usize;
        if pad > 0 && block.as_ptr() != value.as_bytes()[value.len() - 4..].as_ptr() {
            return Err("browser screenshot encoding has invalid padding".into());
        }
        let decode = |byte| TABLE.iter().position(|candidate| *candidate == byte);
        let a = decode(block[0]).ok_or("browser screenshot encoding is invalid")? as u32;
        let b = decode(block[1]).ok_or("browser screenshot encoding is invalid")? as u32;
        let c = if block[2] == b'=' {
            0
        } else {
            decode(block[2]).ok_or("browser screenshot encoding is invalid")? as u32
        };
        let d = if block[3] == b'=' {
            0
        } else {
            decode(block[3]).ok_or("browser screenshot encoding is invalid")? as u32
        };
        if block[2] == b'=' && block[3] != b'='
            || pad == 1 && c & 0b11 != 0
            || pad == 2 && b & 0b1111 != 0
        {
            return Err("browser screenshot encoding is not canonical base64".into());
        }
        let packed = (a << 18) | (b << 12) | (c << 6) | d;
        output.push((packed >> 16) as u8);
        if pad < 2 {
            output.push((packed >> 8) as u8);
        }
        if pad == 0 {
            output.push(packed as u8);
        }
        if output.len() > max_bytes {
            return Err("browser screenshot exceeds its byte bound".into());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::decode_base64;

    #[test]
    fn decoder_rejects_noncanonical_and_oversized_data() {
        assert_eq!(decode_base64("aGVsbG8=", 8).unwrap(), b"hello");
        assert!(decode_base64("aGVsbG8=", 4).is_err());
        assert!(decode_base64("aGVsbG8!", 8).is_err());
        assert!(decode_base64("aGVsbG9=", 8).is_err());
    }
}
