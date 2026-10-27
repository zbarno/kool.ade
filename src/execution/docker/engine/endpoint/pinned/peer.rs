use std::{ffi::c_void, io, mem::size_of, ptr::NonNull};

const DOCKER_DESKTOP_REQUIREMENT: &str = "anchor apple generic and identifier \"com.docker.backend\" and certificate leaf[subject.OU] = \"9BNSXJN65R\"";
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

type CfAllocatorRef = *const c_void;
type CfStringRef = *const c_void;
type CfTypeRef = *const c_void;
type CfDataRef = *const c_void;
type CfDictionaryRef = *const c_void;
type SecCodeRef = *mut c_void;
type SecRequirementRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AuditToken {
    value: [libc::c_uint; 8],
}

#[link(name = "bsm", kind = "dylib")]
unsafe extern "C" {
    fn audit_token_to_pid(token: AuditToken) -> libc::pid_t;
}

impl AuditToken {
    fn pid(self) -> io::Result<libc::pid_t> {
        // Use Apple's audit-token helper rather than baking in token field offsets.
        let pid = unsafe { audit_token_to_pid(self) };
        if pid <= 0 {
            return Err(untrusted_peer());
        }
        Ok(pid)
    }
}

type DictionaryRetain = unsafe extern "C" fn(CfAllocatorRef, CfTypeRef) -> CfTypeRef;
type DictionaryRelease = unsafe extern "C" fn(CfAllocatorRef, CfTypeRef);
type DictionaryCopyDescription = unsafe extern "C" fn(CfTypeRef) -> CfStringRef;
type DictionaryEqual = unsafe extern "C" fn(CfTypeRef, CfTypeRef) -> u8;
type DictionaryHash = unsafe extern "C" fn(CfTypeRef) -> usize;

#[repr(C)]
struct CfDictionaryKeyCallBacks {
    version: isize,
    retain: Option<DictionaryRetain>,
    release: Option<DictionaryRelease>,
    copy_description: Option<DictionaryCopyDescription>,
    equal: Option<DictionaryEqual>,
    hash: Option<DictionaryHash>,
}

#[repr(C)]
struct CfDictionaryValueCallBacks {
    version: isize,
    retain: Option<DictionaryRetain>,
    release: Option<DictionaryRelease>,
    copy_description: Option<DictionaryCopyDescription>,
    equal: Option<DictionaryEqual>,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    #[link_name = "kCFAllocatorDefault"]
    static CF_ALLOCATOR_DEFAULT: CfAllocatorRef;
    #[link_name = "kCFTypeDictionaryKeyCallBacks"]
    static CF_TYPE_DICTIONARY_KEY_CALLBACKS: CfDictionaryKeyCallBacks;
    #[link_name = "kCFTypeDictionaryValueCallBacks"]
    static CF_TYPE_DICTIONARY_VALUE_CALLBACKS: CfDictionaryValueCallBacks;

    fn CFDictionaryCreate(
        allocator: CfAllocatorRef,
        keys: *const CfTypeRef,
        values: *const CfTypeRef,
        count: isize,
        key_callbacks: *const CfDictionaryKeyCallBacks,
        value_callbacks: *const CfDictionaryValueCallBacks,
    ) -> CfDictionaryRef;
    fn CFDataCreate(allocator: CfAllocatorRef, bytes: *const u8, length: isize) -> CfDataRef;
    fn CFStringCreateWithCString(
        allocator: CfAllocatorRef,
        value: *const libc::c_char,
        encoding: u32,
    ) -> CfStringRef;
    fn CFRelease(value: CfTypeRef);
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    #[link_name = "kSecGuestAttributeAudit"]
    static SEC_GUEST_ATTRIBUTE_AUDIT: CfStringRef;

    fn SecCodeCopyGuestWithAttributes(
        host: SecCodeRef,
        attributes: CfDictionaryRef,
        flags: u32,
        guest: *mut SecCodeRef,
    ) -> i32;
    fn SecCodeCheckValidity(code: SecCodeRef, flags: u32, requirement: SecRequirementRef) -> i32;
    fn SecRequirementCreateWithString(
        value: CfStringRef,
        flags: u32,
        requirement: *mut SecRequirementRef,
    ) -> i32;
}

struct CfOwned(NonNull<c_void>);

impl CfOwned {
    fn new(value: *const c_void) -> Option<Self> {
        NonNull::new(value.cast_mut()).map(Self)
    }

    fn as_const(&self) -> *const c_void {
        self.0.as_ptr()
    }

    fn as_mut(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_const()) };
    }
}

pub(super) fn verify_docker_desktop_peer(descriptor: libc::c_int) -> io::Result<()> {
    let (peer_pid, effective_peer_pid) = peer_pids(descriptor)?;
    let audit_token = peer_audit_token(descriptor)?;
    let audit_pid = audit_token.pid()?;
    validate_peer_identity(peer_pid, effective_peer_pid, audit_pid)?;

    verify_signed_peer(&audit_token)?;

    let (peer_pid_after, effective_peer_pid_after) = peer_pids(descriptor)?;
    validate_peer_identity(peer_pid_after, effective_peer_pid_after, audit_pid)?;
    Ok(())
}

fn peer_pids(descriptor: libc::c_int) -> io::Result<(libc::pid_t, libc::pid_t)> {
    let pid = peer_pid_option(descriptor, libc::LOCAL_PEERPID)?;
    let effective_pid = peer_pid_option(descriptor, libc::LOCAL_PEEREPID)?;
    Ok((pid, effective_pid))
}

fn peer_pid_option(descriptor: libc::c_int, option: libc::c_int) -> io::Result<libc::pid_t> {
    let mut pid = 0;
    let mut length = size_of::<libc::pid_t>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            descriptor,
            libc::SOL_LOCAL,
            option,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != size_of::<libc::pid_t>() || pid <= 0 {
        return Err(untrusted_peer());
    }
    Ok(pid)
}

fn peer_audit_token(descriptor: libc::c_int) -> io::Result<AuditToken> {
    let mut token = AuditToken { value: [0; 8] };
    let mut length = size_of::<AuditToken>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            descriptor,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            (&mut token as *mut AuditToken).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != size_of::<AuditToken>() {
        return Err(untrusted_peer());
    }
    token.pid()?;
    Ok(token)
}

fn validate_peer_identity(
    peer_pid: libc::pid_t,
    effective_peer_pid: libc::pid_t,
    audit_pid: libc::pid_t,
) -> io::Result<()> {
    if peer_pid <= 0 || peer_pid != effective_peer_pid || peer_pid != audit_pid {
        return Err(untrusted_peer());
    }
    Ok(())
}

fn verify_signed_peer(audit_token: &AuditToken) -> io::Result<()> {
    let audit_data = unsafe {
        CfOwned::new(CFDataCreate(
            CF_ALLOCATOR_DEFAULT,
            audit_token.value.as_ptr().cast(),
            size_of::<AuditToken>() as isize,
        ))
    }
    .ok_or_else(untrusted_peer)?;
    let attribute_key = unsafe { SEC_GUEST_ATTRIBUTE_AUDIT };
    if attribute_key.is_null() {
        return Err(untrusted_peer());
    }
    let keys = [attribute_key];
    let values = [audit_data.as_const()];
    let attributes = unsafe {
        CfOwned::new(CFDictionaryCreate(
            CF_ALLOCATOR_DEFAULT,
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &CF_TYPE_DICTIONARY_KEY_CALLBACKS,
            &CF_TYPE_DICTIONARY_VALUE_CALLBACKS,
        ))
    }
    .ok_or_else(untrusted_peer)?;

    let requirement_text =
        std::ffi::CString::new(DOCKER_DESKTOP_REQUIREMENT).map_err(|_| untrusted_peer())?;
    let requirement_text = unsafe {
        CfOwned::new(CFStringCreateWithCString(
            CF_ALLOCATOR_DEFAULT,
            requirement_text.as_ptr(),
            K_CF_STRING_ENCODING_UTF8,
        ))
    }
    .ok_or_else(untrusted_peer)?;
    let mut requirement = std::ptr::null_mut();
    let status =
        unsafe { SecRequirementCreateWithString(requirement_text.as_const(), 0, &mut requirement) };
    if status != 0 {
        return Err(untrusted_peer());
    }
    let requirement = CfOwned::new(requirement).ok_or_else(untrusted_peer)?;

    let mut guest = std::ptr::null_mut();
    let status = unsafe {
        SecCodeCopyGuestWithAttributes(std::ptr::null_mut(), attributes.as_const(), 0, &mut guest)
    };
    if status != 0 {
        return Err(untrusted_peer());
    }
    let guest = CfOwned::new(guest).ok_or_else(untrusted_peer)?;
    let status = unsafe { SecCodeCheckValidity(guest.as_mut(), 0, requirement.as_mut()) };
    if status != 0 {
        return Err(untrusted_peer());
    }
    Ok(())
}

fn untrusted_peer() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The connected Unix socket peer is not verified Docker Desktop",
    )
}

#[cfg(test)]
mod tests {
    use super::{AuditToken, validate_peer_identity};

    fn audit_token(pid: u32) -> AuditToken {
        let mut value = [0; 8];
        value[5] = pid;
        AuditToken { value }
    }

    #[test]
    fn reads_pid_from_the_audit_token_pid_slot() {
        assert_eq!(audit_token(42).pid().unwrap(), 42);
    }

    #[test]
    fn rejects_audit_tokens_without_a_valid_pid() {
        assert!(audit_token(0).pid().is_err());
        assert!(audit_token(u32::MAX).pid().is_err());
    }

    #[test]
    fn requires_peer_and_effective_peer_pid_to_match_the_audit_token() {
        let audit_pid = audit_token(42).pid().unwrap();
        let different_audit_pid = audit_token(43).pid().unwrap();
        assert!(validate_peer_identity(42, 42, audit_pid).is_ok());
        assert!(validate_peer_identity(42, 43, audit_pid).is_err());
        assert!(validate_peer_identity(42, 42, different_audit_pid).is_err());
        assert!(validate_peer_identity(0, 0, 0).is_err());
    }
}
