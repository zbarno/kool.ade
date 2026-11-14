use super::*;
use crate::error::AppError;

#[test]
fn connecting_missing_path_errors_friendly() {
    match attempt_connect("") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}

#[test]
fn connecting_nonexistent_path_errors_friendly() {
    match attempt_connect("/no/such/dir-xyz-123") {
        Ok(_) => panic!("expected connection error"),
        Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
    }
}
