use std::mem::{offset_of, size_of, zeroed};

use windows_sys::Win32::{
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, GetAce,
        GetAclInformation, IsWellKnownSid, PSID, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
        ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
        ACCESS_ALLOWED_OBJECT_ACE_TYPE,
    },
};

use super::verify_sid;

pub fn verify_no_untrusted_delete_rights(dacl: *mut ACL, current_user: PSID) -> anyhow::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DELETE_CHILD, WRITE_DAC, WRITE_OWNER,
    };

    anyhow::ensure!(
        !dacl.is_null() && !current_user.is_null(),
        "machine setup ancestor ACL is incomplete"
    );
    let mut info: ACL_SIZE_INFORMATION = unsafe { zeroed() };
    anyhow::ensure!(
        unsafe {
            GetAclInformation(
                dacl,
                &mut info as *mut _ as *mut core::ffi::c_void,
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } != 0,
        "could not inspect machine setup ancestor ACL"
    );
    let acl_size = usize::from(unsafe { (*dacl).AclSize });
    let acl_bytes_in_use = info.AclBytesInUse as usize;
    anyhow::ensure!(
        acl_size >= size_of::<ACL>()
            && acl_bytes_in_use >= size_of::<ACL>()
            && acl_bytes_in_use <= acl_size
            && info.AceCount == unsafe { (*dacl).AceCount } as u32,
        "machine setup ancestor ACL size metadata is invalid"
    );
    // SAFETY: GetSecurityInfo returned this ACL and GetAclInformation bounded
    // its initialized contents within the ACL's declared allocation.
    let acl_bytes = unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), acl_bytes_in_use) };
    let mut cursor = size_of::<ACL>();
    for index in 0..info.AceCount {
        let mut raw_ace: *mut core::ffi::c_void = std::ptr::null_mut();
        anyhow::ensure!(
            unsafe { GetAce(dacl, index, &mut raw_ace) } != 0 && !raw_ace.is_null(),
            "could not read a machine setup ancestor ACL entry"
        );
        let expected = (dacl as usize)
            .checked_add(cursor)
            .ok_or_else(|| anyhow::anyhow!("machine setup ancestor ACL offset overflowed"))?;
        anyhow::ensure!(
            raw_ace as usize == expected,
            "machine setup ancestor ACL entries are not contiguous"
        );
        let remaining = acl_bytes
            .get(cursor..)
            .ok_or_else(|| anyhow::anyhow!("machine setup ancestor ACL entry is out of bounds"))?;
        anyhow::ensure!(
            remaining.len() >= size_of::<ACE_HEADER>(),
            "machine setup ancestor ACL has a truncated ACE header"
        );
        // SAFETY: the header fits in the bounded ACL byte slice above.
        let header = unsafe { &*(raw_ace as *const ACE_HEADER) };
        let ace_size = usize::from(header.AceSize);
        anyhow::ensure!(
            ace_size >= size_of::<ACE_HEADER>()
                && ace_size.is_multiple_of(size_of::<u32>())
                && ace_size <= remaining.len(),
            "machine setup ancestor ACL has an invalid ACE size"
        );
        let ace_bytes = &remaining[..ace_size];
        let ace_type = u32::from(header.AceType);
        match ace_type {
            ACCESS_ALLOWED_ACE_TYPE => {
                anyhow::ensure!(
                    ace_size >= size_of::<ACCESS_ALLOWED_ACE>(),
                    "machine setup ancestor ACL has a truncated allow entry"
                );
                let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
                let sid_bytes = ace_bytes
                    .get(sid_offset..)
                    .ok_or_else(|| anyhow::anyhow!("machine setup ancestor SID is truncated"))?;
                let sid = sid_bytes.as_ptr().cast_mut().cast();
                verify_sid(sid, sid_bytes, "machine setup ancestor ACL entry")?;
                // SAFETY: the ACE and mask fit in the validated slice above.
                let allow = unsafe { &*(raw_ace as *const ACCESS_ALLOWED_ACE) };
                let trusted = unsafe {
                    windows_sys::Win32::Security::EqualSid(sid, current_user) != 0
                        || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                        || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                };
                let inherit_only = u32::from(header.AceFlags)
                    & windows_sys::Win32::Security::INHERIT_ONLY_ACE
                    != 0;
                let replace_rights = DELETE
                    | FILE_DELETE_CHILD
                    | WRITE_DAC
                    | WRITE_OWNER
                    | windows_sys::Win32::Foundation::GENERIC_ALL;
                anyhow::ensure!(
                    trusted || inherit_only || allow.Mask & replace_rights == 0,
                    "machine setup ancestor ACL allows another principal to replace protected state"
                );
            }
            ACCESS_ALLOWED_COMPOUND_ACE_TYPE
            | ACCESS_ALLOWED_OBJECT_ACE_TYPE
            | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
            | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE => {
                anyhow::bail!("machine setup ancestor ACL contains an unsupported allow entry");
            }
            _ => {}
        }
        cursor = cursor
            .checked_add(ace_size)
            .ok_or_else(|| anyhow::anyhow!("machine setup ancestor ACL offset overflowed"))?;
    }
    anyhow::ensure!(
        cursor == acl_bytes.len(),
        "machine setup ancestor ACL size does not match its entries"
    );
    Ok(())
}
