use std::mem::{align_of, offset_of, size_of, zeroed};

use windows_sys::Win32::{
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, GetAce,
        GetAclInformation, GetLengthSid, GetSecurityDescriptorLength, IsValidSid, IsWellKnownSid,
        PSECURITY_DESCRIPTOR, PSID, SECURITY_DESCRIPTOR, SID, WinBuiltinAdministratorsSid,
        WinCreatorOwnerSid, WinLocalSystemSid,
    },
    System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
        ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
        ACCESS_ALLOWED_OBJECT_ACE_TYPE, SYSTEM_ACCESS_FILTER_ACE_TYPE,
    },
};

mod ancestors;
pub(super) use ancestors::verify_no_untrusted_delete_rights;

pub(super) fn verify_private_acl(
    descriptor: PSECURITY_DESCRIPTOR,
    owner: PSID,
    dacl: *mut ACL,
    current_user: PSID,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !descriptor.is_null() && !owner.is_null() && !dacl.is_null() && !current_user.is_null(),
        "machine setup state ACL contains a missing descriptor component"
    );
    let descriptor_length = unsafe { GetSecurityDescriptorLength(descriptor) } as usize;
    anyhow::ensure!(
        descriptor_length >= size_of::<SECURITY_DESCRIPTOR>()
            && descriptor_length <= isize::MAX as usize,
        "machine setup state security descriptor has an invalid size"
    );
    let descriptor_start = descriptor as usize;
    let descriptor_end = descriptor_start
        .checked_add(descriptor_length)
        .ok_or_else(|| anyhow::anyhow!("machine setup state descriptor size overflowed"))?;
    // SAFETY: GetNamedSecurityInfoW succeeded and returned a LocalFree-owned
    // descriptor. GetSecurityDescriptorLength reports the complete allocation.
    let descriptor_bytes =
        unsafe { std::slice::from_raw_parts(descriptor.cast::<u8>(), descriptor_length) };

    let acl_start = dacl as usize;
    anyhow::ensure!(
        acl_start.is_multiple_of(align_of::<ACL>()),
        "machine setup state DACL is misaligned"
    );
    let acl_start_offset = acl_start
        .checked_sub(descriptor_start)
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL is outside its descriptor"))?;
    let acl_header_end = acl_start_offset
        .checked_add(size_of::<ACL>())
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL header size overflowed"))?;
    let acl_header = descriptor_bytes
        .get(acl_start_offset..acl_header_end)
        .ok_or_else(|| {
            anyhow::anyhow!("machine setup state DACL header is outside its descriptor")
        })?;
    let acl_size_offset = offset_of!(ACL, AclSize);
    let acl_size = usize::from(u16::from_ne_bytes([
        acl_header[acl_size_offset],
        acl_header[acl_size_offset + 1],
    ]));
    let acl_ace_count_offset = offset_of!(ACL, AceCount);
    let acl_ace_count = u32::from(u16::from_ne_bytes([
        acl_header[acl_ace_count_offset],
        acl_header[acl_ace_count_offset + 1],
    ]));
    anyhow::ensure!(
        acl_size >= size_of::<ACL>() && acl_size.is_multiple_of(size_of::<u32>()),
        "machine setup state DACL has an invalid size"
    );
    let acl_end_offset = acl_start_offset
        .checked_add(acl_size)
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL size overflowed"))?;
    let acl_end = acl_start
        .checked_add(acl_size)
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL address overflowed"))?;
    anyhow::ensure!(
        acl_end_offset <= descriptor_bytes.len() && acl_end <= descriptor_end,
        "machine setup state DACL is outside its descriptor"
    );

    let owner_offset = (owner as usize)
        .checked_sub(descriptor_start)
        .ok_or_else(|| anyhow::anyhow!("machine setup state owner is outside its descriptor"))?;
    anyhow::ensure!(
        (owner as usize).is_multiple_of(align_of::<u32>()),
        "machine setup state owner SID is misaligned"
    );
    let owner_bytes = descriptor_bytes
        .get(owner_offset..)
        .ok_or_else(|| anyhow::anyhow!("machine setup state owner is outside its descriptor"))?;
    verify_sid(owner, owner_bytes, "machine setup state owner")?;
    anyhow::ensure!(
        unsafe { windows_sys::Win32::Security::EqualSid(owner, current_user) } != 0,
        "machine setup state is not owned by the current Windows user"
    );

    let mut info: ACL_SIZE_INFORMATION = unsafe { zeroed() };
    let acl_info_ok = unsafe {
        GetAclInformation(
            dacl,
            &mut info as *mut _ as *mut core::ffi::c_void,
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    anyhow::ensure!(
        acl_info_ok != 0,
        "could not inspect machine setup state DACL"
    );
    let acl_bytes_in_use = info.AclBytesInUse as usize;
    let acl_bytes_free = info.AclBytesFree as usize;
    anyhow::ensure!(
        acl_bytes_in_use >= size_of::<ACL>()
            && acl_bytes_in_use <= acl_size
            && acl_bytes_in_use.checked_add(acl_bytes_free) == Some(acl_size)
            && info.AceCount == acl_ace_count,
        "machine setup state DACL size metadata is inconsistent"
    );
    let acl_used_end_offset = acl_start_offset
        .checked_add(acl_bytes_in_use)
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL used size overflowed"))?;
    let acl_bytes = descriptor_bytes
        .get(acl_start_offset..acl_used_end_offset)
        .ok_or_else(|| anyhow::anyhow!("machine setup state DACL contents are truncated"))?;
    // The returned DACL includes inherited ACEs. Check them alongside explicit
    // entries so a permissive parent cannot make this path public.
    let mut ace_cursor = size_of::<ACL>();
    for index in 0..info.AceCount {
        let mut raw_ace: *mut core::ffi::c_void = std::ptr::null_mut();
        let get_ace_ok = unsafe { GetAce(dacl, index, &mut raw_ace) };
        anyhow::ensure!(
            get_ace_ok != 0 && !raw_ace.is_null(),
            "could not read a machine setup state ACL entry"
        );
        let ace_address = raw_ace as usize;
        let expected_ace_address = descriptor_start
            .checked_add(acl_start_offset)
            .and_then(|start| start.checked_add(ace_cursor))
            .ok_or_else(|| anyhow::anyhow!("machine setup state ACL entry offset overflowed"))?;
        anyhow::ensure!(
            ace_address == expected_ace_address,
            "machine setup state ACL entries are not contiguous"
        );
        let remaining_ace_bytes = acl_bytes
            .get(ace_cursor..)
            .ok_or_else(|| anyhow::anyhow!("machine setup state ACL entry is outside the DACL"))?;
        let header_size = size_of::<ACE_HEADER>();
        anyhow::ensure!(
            remaining_ace_bytes.len() >= header_size,
            "machine setup state DACL contains a truncated ACE header"
        );
        let ace_size_offset = offset_of!(ACE_HEADER, AceSize);
        let ace_size = usize::from(u16::from_ne_bytes([
            remaining_ace_bytes[ace_size_offset],
            remaining_ace_bytes[ace_size_offset + 1],
        ]));
        anyhow::ensure!(
            ace_size >= header_size
                && ace_size.is_multiple_of(size_of::<u32>())
                && ace_size <= remaining_ace_bytes.len(),
            "machine setup state DACL contains an invalid ACE size"
        );
        let ace_bytes = &remaining_ace_bytes[..ace_size];
        let ace_type = u32::from(ace_bytes[offset_of!(ACE_HEADER, AceType)]);
        if ace_type != ACCESS_ALLOWED_ACE_TYPE {
            anyhow::ensure!(
                !matches!(
                    ace_type,
                    ACCESS_ALLOWED_COMPOUND_ACE_TYPE
                        | ACCESS_ALLOWED_OBJECT_ACE_TYPE
                        | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
                        | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
                ),
                "machine setup state DACL contains an unsupported allow ACE"
            );
            anyhow::ensure!(
                ace_type <= SYSTEM_ACCESS_FILTER_ACE_TYPE,
                "machine setup state DACL contains an unknown ACE type"
            );
            ace_cursor = ace_cursor
                .checked_add(ace_size)
                .ok_or_else(|| anyhow::anyhow!("machine setup state ACL entry size overflowed"))?;
            continue;
        }
        anyhow::ensure!(
            ace_size >= size_of::<ACCESS_ALLOWED_ACE>(),
            "machine setup state DACL contains a truncated allow ACE"
        );
        let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let sid_bytes = ace_bytes
            .get(sid_offset..)
            .ok_or_else(|| anyhow::anyhow!("machine setup state DACL contains a truncated SID"))?;
        let ace_sid = sid_bytes.as_ptr().cast_mut().cast();
        verify_sid(ace_sid, sid_bytes, "machine setup state DACL entry")?;
        let is_current_user =
            unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, current_user) } != 0;
        let is_system_or_admin = unsafe {
            IsWellKnownSid(ace_sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(ace_sid, WinBuiltinAdministratorsSid) != 0
        };
        let is_creator_owner_inherit_only = u32::from(ace_bytes[offset_of!(ACE_HEADER, AceFlags)])
            & windows_sys::Win32::Security::INHERIT_ONLY_ACE
            != 0
            && unsafe { IsWellKnownSid(ace_sid, WinCreatorOwnerSid) } != 0;
        anyhow::ensure!(
            is_current_user || is_system_or_admin || is_creator_owner_inherit_only,
            "machine setup state DACL grants access to another Windows principal"
        );
        ace_cursor = ace_cursor
            .checked_add(ace_size)
            .ok_or_else(|| anyhow::anyhow!("machine setup state ACL entry size overflowed"))?;
    }
    anyhow::ensure!(
        ace_cursor == acl_bytes.len(),
        "machine setup state DACL used size does not match its entries"
    );
    Ok(())
}

pub(super) fn verify_sid(sid: PSID, sid_bytes: &[u8], description: &str) -> anyhow::Result<()> {
    let subauthority_count_offset = offset_of!(SID, SubAuthorityCount);
    let subauthority_offset = offset_of!(SID, SubAuthority);
    anyhow::ensure!(
        sid_bytes.len() >= subauthority_offset,
        "{description} contains a truncated SID header"
    );
    let subauthority_count = usize::from(sid_bytes[subauthority_count_offset]);
    let sid_length = subauthority_count
        .checked_mul(size_of::<u32>())
        .and_then(|length| subauthority_offset.checked_add(length))
        .ok_or_else(|| anyhow::anyhow!("{description} SID length overflowed"))?;
    anyhow::ensure!(
        sid_length <= sid_bytes.len(),
        "{description} contains a truncated SID"
    );
    anyhow::ensure!(
        unsafe { IsValidSid(sid) } != 0,
        "{description} contains an invalid SID"
    );
    anyhow::ensure!(
        unsafe { GetLengthSid(sid) } as usize == sid_length,
        "{description} SID size is inconsistent"
    );
    Ok(())
}
