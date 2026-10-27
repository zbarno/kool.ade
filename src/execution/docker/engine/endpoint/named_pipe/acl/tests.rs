use super::{AclEntry, Trustee, trusted_pipe_acl};
use windows_sys::Win32::Foundation::{GENERIC_ALL, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, WRITE_DAC, WRITE_OWNER,
};

#[test]
fn rejects_missing_caller_access_or_broad_write_access() {
    assert!(!trusted_pipe_acl(true, true, false, &[]));
    assert!(!trusted_pipe_acl(
        true,
        true,
        false,
        &[AclEntry {
            trustee: Trustee::CurrentUser,
            mask: FILE_GENERIC_READ,
        }]
    ));
    assert!(!trusted_pipe_acl(
        true,
        true,
        false,
        &[
            AclEntry {
                trustee: Trustee::CurrentUser,
                mask: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            },
            AclEntry {
                trustee: Trustee::BroadReadOnly,
                mask: FILE_GENERIC_WRITE,
            },
        ]
    ));
    for broad_right in [DELETE, GENERIC_WRITE, GENERIC_ALL, WRITE_DAC, WRITE_OWNER] {
        assert!(!trusted_pipe_acl(
            true,
            true,
            false,
            &[
                AclEntry {
                    trustee: Trustee::CurrentUser,
                    mask: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                },
                AclEntry {
                    trustee: Trustee::BroadReadOnly,
                    mask: broad_right,
                },
            ]
        ));
    }
}

#[test]
fn permits_default_broad_read_only_entries() {
    assert!(trusted_pipe_acl(
        true,
        true,
        false,
        &[
            AclEntry {
                trustee: Trustee::CurrentUser,
                mask: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            },
            AclEntry {
                trustee: Trustee::BroadReadOnly,
                mask: FILE_GENERIC_READ,
            },
        ]
    ));
}

#[test]
fn rejects_unrecognized_acl_principals() {
    assert!(!trusted_pipe_acl(
        true,
        true,
        false,
        &[
            AclEntry {
                trustee: Trustee::CurrentUser,
                mask: FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            },
            AclEntry {
                trustee: Trustee::Other,
                mask: FILE_GENERIC_READ,
            },
        ]
    ));
}
