use super::super::super::failure;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

pub(super) fn parse_capacity_output(output: &[u8]) -> Result<u64, PrerequisiteFailure> {
    let output = std::str::from_utf8(output).map_err(|_| capacity_output_failure())?;
    let mut lines = output.lines();
    if lines.next() != Some("Type Avail") {
        return Err(capacity_output_failure());
    }
    let mut fields = lines
        .next()
        .ok_or_else(capacity_output_failure)?
        .split_ascii_whitespace();
    if fields.next() != Some("overlay") {
        return Err(capacity_output_failure());
    }
    let available = fields
        .next()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(capacity_output_failure)?;
    if fields.next().is_some() || lines.next().is_some() {
        return Err(capacity_output_failure());
    }
    Ok(available)
}

pub(super) fn capacity_output_failure() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::InsufficientDisk,
        "Docker Desktop did not return a valid Linux VM overlay-storage capacity sample",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::parse_capacity_output;

    #[test]
    fn parses_only_bounded_overlay_free_bytes() {
        assert_eq!(
            parse_capacity_output(b"Type Avail\noverlay 123456\n").unwrap(),
            123_456
        );
    }

    #[test]
    fn rejects_unknown_filesystems_malformed_and_overflowed_capacity() {
        for output in [
            b"Type Avail\next4 123456\n".as_slice(),
            b"Type Avail\noverlay nope\n".as_slice(),
            b"Type Avail\noverlay 18446744073709551616\n".as_slice(),
            b"Type Avail\noverlay 123\nextra\n".as_slice(),
            b"Type Avail\noverlay 123\nmalicious output\n".as_slice(),
        ] {
            assert!(parse_capacity_output(output).is_err());
        }
    }
}
