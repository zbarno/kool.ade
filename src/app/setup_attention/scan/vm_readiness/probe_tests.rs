use std::io::Cursor;

use super::*;

#[test]
fn retained_qemu_output_is_bounded_and_overflow_is_reported() {
    let output = vec![b'x'; MAX_RETAINED_OUTPUT + 17];
    let (retained, overflowed) = retain_bounded(Cursor::new(output)).unwrap();
    assert_eq!(retained.len(), MAX_RETAINED_OUTPUT);
    assert!(overflowed);
}

#[test]
fn retained_qemu_output_preserves_small_payloads() {
    let (retained, overflowed) = retain_bounded(Cursor::new(b"QEMU version\n")).unwrap();
    assert_eq!(retained, b"QEMU version\n");
    assert!(!overflowed);
}
