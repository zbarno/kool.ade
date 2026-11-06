pub(super) fn pid_line(pid: u32) -> ([u8; 11], usize) {
    let mut digits = [0_u8; 11];
    digits[10] = b'\n';
    let mut cursor = 10;
    let mut value = pid;
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let count = 11 - cursor;
    digits.copy_within(cursor.., 0);
    (digits, count)
}

#[cfg(test)]
mod tests {
    use super::pid_line;

    #[test]
    fn child_pid_is_encoded_as_one_cgroup_procs_line() {
        let (bytes, count) = pid_line(1234);
        assert_eq!(&bytes[..count], b"1234\n");
        let (maximum, count) = pid_line(u32::MAX);
        assert_eq!(&maximum[..count], b"4294967295\n");
    }
}
