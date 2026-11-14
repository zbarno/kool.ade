//! Deterministic Windows VM storage invariants, shared with the native adapter.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowsStorageBlocker {
    NotAbsolute,
    NotDirectory,
    ReparsePoint,
    WrongOwner,
    UnrestrictedAcl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowsStorageFacts {
    pub(super) absolute: bool,
    pub(super) directory: bool,
    pub(super) reparse_point: bool,
    pub(super) current_user_owner: bool,
    pub(super) private_acl: bool,
}

pub(super) fn assess_storage_facts(
    facts: WindowsStorageFacts,
) -> Result<(), WindowsStorageBlocker> {
    if !facts.absolute {
        return Err(WindowsStorageBlocker::NotAbsolute);
    }
    if !facts.directory {
        return Err(WindowsStorageBlocker::NotDirectory);
    }
    if facts.reparse_point {
        return Err(WindowsStorageBlocker::ReparsePoint);
    }
    if !facts.current_user_owner {
        return Err(WindowsStorageBlocker::WrongOwner);
    }
    if !facts.private_acl {
        return Err(WindowsStorageBlocker::UnrestrictedAcl);
    }
    Ok(())
}

pub(super) fn valid_session_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value)
}

pub(super) fn is_fixed_local_volume(drive_type: u32) -> bool {
    // Win32 DRIVE_FIXED. In particular DRIVE_REMOTE is rejected for mapped SMB paths.
    drive_type == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_facts() -> WindowsStorageFacts {
        WindowsStorageFacts {
            absolute: true,
            directory: true,
            reparse_point: false,
            current_user_owner: true,
            private_acl: true,
        }
    }

    #[test]
    fn storage_path_requires_an_absolute_owned_non_reparse_private_directory() {
        assert_eq!(assess_storage_facts(safe_facts()), Ok(()));
        for (facts, blocker) in [
            (
                WindowsStorageFacts {
                    absolute: false,
                    ..safe_facts()
                },
                WindowsStorageBlocker::NotAbsolute,
            ),
            (
                WindowsStorageFacts {
                    directory: false,
                    ..safe_facts()
                },
                WindowsStorageBlocker::NotDirectory,
            ),
            (
                WindowsStorageFacts {
                    reparse_point: true,
                    ..safe_facts()
                },
                WindowsStorageBlocker::ReparsePoint,
            ),
            (
                WindowsStorageFacts {
                    current_user_owner: false,
                    ..safe_facts()
                },
                WindowsStorageBlocker::WrongOwner,
            ),
            (
                WindowsStorageFacts {
                    private_acl: false,
                    ..safe_facts()
                },
                WindowsStorageBlocker::UnrestrictedAcl,
            ),
        ] {
            assert_eq!(assess_storage_facts(facts), Err(blocker));
        }
    }

    #[test]
    fn session_directory_names_are_canonical_uuids() {
        assert!(valid_session_id("123e4567-e89b-12d3-a456-426614174000"));
        for invalid in [
            "",
            "../123e4567-e89b-12d3-a456-426614174000",
            "123E4567-E89B-12D3-A456-426614174000",
            "123e4567-e89b-12d3-a456-426614174000/child",
        ] {
            assert!(!valid_session_id(invalid), "accepted {invalid:?}");
        }
    }

    #[test]
    fn local_vm_storage_accepts_only_fixed_volumes() {
        assert!(is_fixed_local_volume(3));
        for drive_type in [0, 2, 4, 5, 6] {
            assert!(!is_fixed_local_volume(drive_type));
        }
    }
}
