use std::mem::{align_of, offset_of, size_of, zeroed};

use windows_sys::Win32::{
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, GetAce,
        GetAclInformation, GetSecurityDescriptorLength, IsWellKnownSid, PSECURITY_DESCRIPTOR, PSID,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
        ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
        ACCESS_ALLOWED_OBJECT_ACE_TYPE,
    },
};

use super::verify_sid;

pub fn verify_no_untrusted_delete_rights(
    descriptor: PSECURITY_DESCRIPTOR,
    dacl: *mut ACL,
    current_user: PSID,
) -> anyhow::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DELETE_CHILD, WRITE_DAC, WRITE_OWNER,
    };

    anyhow::ensure!(
        !descriptor.is_null() && !dacl.is_null() && !current_user.is_null(),
        "machine setup ancestor ACL is incomplete"
    );
    let descriptor_length = unsafe { GetSecurityDescriptorLength(descriptor) } as usize;
    anyhow::ensure!(
        descriptor_length >= size_of::<windows_sys::Win32::Security::SECURITY_DESCRIPTOR>()
            && descriptor_length <= isize::MAX as usize,
        "machine setup ancestor security descriptor has an invalid size"
    );
    let descriptor_start = descriptor as usize;
    let descriptor_end = descriptor_start
        .checked_add(descriptor_length)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor descriptor size overflowed"))?;
    // SAFETY: GetSecurityInfo succeeded and returned this LocalFree-owned
    // descriptor. GetSecurityDescriptorLength reports its complete allocation.
    let descriptor_bytes =
        unsafe { std::slice::from_raw_parts(descriptor.cast::<u8>(), descriptor_length) };
    let dacl_start = dacl as usize;
    anyhow::ensure!(
        dacl_start.is_multiple_of(align_of::<ACL>()),
        "machine setup ancestor DACL is misaligned"
    );
    let dacl_offset = dacl_start
        .checked_sub(descriptor_start)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL is outside its descriptor"))?;
    let dacl_header_end = dacl_offset
        .checked_add(size_of::<ACL>())
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL header size overflowed"))?;
    let dacl_header = descriptor_bytes
        .get(dacl_offset..dacl_header_end)
        .ok_or_else(|| {
            anyhow::anyhow!("machine setup ancestor DACL header is outside its descriptor")
        })?;
    let acl_size_offset = offset_of!(ACL, AclSize);
    let acl_size = usize::from(u16::from_ne_bytes([
        dacl_header[acl_size_offset],
        dacl_header[acl_size_offset + 1],
    ]));
    let ace_count_offset = offset_of!(ACL, AceCount);
    let acl_ace_count = u32::from(u16::from_ne_bytes([
        dacl_header[ace_count_offset],
        dacl_header[ace_count_offset + 1],
    ]));
    anyhow::ensure!(
        acl_size >= size_of::<ACL>() && acl_size.is_multiple_of(size_of::<u32>()),
        "machine setup ancestor DACL has an invalid size"
    );
    let dacl_end = dacl_start
        .checked_add(acl_size)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL address overflowed"))?;
    let dacl_end_offset = dacl_offset
        .checked_add(acl_size)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL size overflowed"))?;
    anyhow::ensure!(
        dacl_end <= descriptor_end && dacl_end_offset <= descriptor_bytes.len(),
        "machine setup ancestor DACL is outside its descriptor"
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
    let acl_bytes_in_use = info.AclBytesInUse as usize;
    let acl_bytes_free = info.AclBytesFree as usize;
    anyhow::ensure!(
        acl_size >= size_of::<ACL>()
            && acl_bytes_in_use >= size_of::<ACL>()
            && acl_bytes_in_use <= acl_size
            && acl_bytes_in_use.checked_add(acl_bytes_free) == Some(acl_size)
            && info.AceCount == acl_ace_count,
        "machine setup ancestor ACL size metadata is invalid"
    );
    // SAFETY: GetSecurityInfo returned this ACL and GetAclInformation bounded
    // its initialized contents within the ACL's declared allocation.
    let acl_used_end = dacl_offset
        .checked_add(acl_bytes_in_use)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL contents overflowed"))?;
    let acl_bytes = descriptor_bytes
        .get(dacl_offset..acl_used_end)
        .ok_or_else(|| anyhow::anyhow!("machine setup ancestor DACL contents are truncated"))?;
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
        let header_ace_size_offset = offset_of!(ACE_HEADER, AceSize);
        let ace_size = usize::from(u16::from_ne_bytes([
            remaining[header_ace_size_offset],
            remaining[header_ace_size_offset + 1],
        ]));
        anyhow::ensure!(
            ace_size >= size_of::<ACE_HEADER>()
                && ace_size.is_multiple_of(size_of::<u32>())
                && ace_size <= remaining.len(),
            "machine setup ancestor ACL has an invalid ACE size"
        );
        let ace_bytes = &remaining[..ace_size];
        let ace_type = u32::from(ace_bytes[offset_of!(ACE_HEADER, AceType)]);
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
                let mask_offset = offset_of!(ACCESS_ALLOWED_ACE, Mask);
                let allow_mask = u32::from_ne_bytes([
                    ace_bytes[mask_offset],
                    ace_bytes[mask_offset + 1],
                    ace_bytes[mask_offset + 2],
                    ace_bytes[mask_offset + 3],
                ]);
                let trusted = unsafe {
                    windows_sys::Win32::Security::EqualSid(sid, current_user) != 0
                        || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                        || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                };
                let inherit_only = u32::from(ace_bytes[offset_of!(ACE_HEADER, AceFlags)])
                    & windows_sys::Win32::Security::INHERIT_ONLY_ACE
                    != 0;
                let replace_rights = DELETE
                    | FILE_DELETE_CHILD
                    | WRITE_DAC
                    | WRITE_OWNER
                    | windows_sys::Win32::Foundation::GENERIC_ALL;
                anyhow::ensure!(
                    trusted || inherit_only || allow_mask & replace_rights == 0,
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
