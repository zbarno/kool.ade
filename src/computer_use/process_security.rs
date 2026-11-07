use std::io;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

/// Keep a spawned probe in its private process group and prevent exec privilege
/// gains or syscalls that could move descendants into another session/group.
pub(crate) fn install_child_restrictions() -> io::Result<()> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        const BPF_LD_W_ABS: u16 = 0x20;
        const BPF_JMP_JEQ_K: u16 = 0x15;
        const BPF_JMP_JSET_K: u16 = 0x45;
        const BPF_RET_K: u16 = 0x06;
        const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
        const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
        const ARCH_X86_64: u32 = 0xc000_003e;
        // The x32 ABI shares AUDIT_ARCH_X86_64 and marks syscall numbers with
        // this bit, so reject it before matching native syscall numbers.
        let deny = SECCOMP_RET_ERRNO | libc::EPERM as u32;
        let mut filter = [
            libc::sock_filter {
                code: BPF_LD_W_ABS,
                jt: 0,
                jf: 0,
                k: 4,
            },
            libc::sock_filter {
                code: BPF_JMP_JEQ_K,
                jt: 1,
                jf: 0,
                k: ARCH_X86_64,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: deny,
            },
            libc::sock_filter {
                code: BPF_LD_W_ABS,
                jt: 0,
                jf: 0,
                k: 0,
            },
            libc::sock_filter {
                code: BPF_JMP_JSET_K,
                jt: 0,
                jf: 1,
                k: X32_SYSCALL_BIT,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: deny,
            },
            libc::sock_filter {
                code: BPF_JMP_JEQ_K,
                jt: 0,
                jf: 1,
                k: libc::SYS_setsid as u32,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: deny,
            },
            libc::sock_filter {
                code: BPF_JMP_JEQ_K,
                jt: 0,
                jf: 1,
                k: libc::SYS_setpgid as u32,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: deny,
            },
            libc::sock_filter {
                code: BPF_JMP_JEQ_K,
                jt: 0,
                jf: 1,
                k: libc::SYS_unshare as u32,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: deny,
            },
            libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ALLOW,
            },
        ];
        let mut program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        // SAFETY: these prctl calls pass scalar arguments and a live sock_fprog
        // whose filter array remains valid until the kernel copies it.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                (&mut program as *mut libc::sock_fprog) as libc::c_ulong,
                0,
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "isolated readiness process restrictions require x86_64 Linux",
        ))
    }
}

/// Confirm the current host can install the exact restrictions used by VM
/// children. The probe runs only a fixed, trusted system utility in a child.
pub(crate) fn verify_child_restrictions() -> io::Result<()> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        use std::{os::unix::process::CommandExt, process::Command};
        let executable = std::path::Path::new("/usr/bin/true");
        crate::computer_use::validate_trusted_system_executable(executable)?;
        let mut command = Command::new(executable);
        // SAFETY: the child callback only installs no-new-privileges and a
        // fixed seccomp filter before exec; it accesses no shared Rust state.
        unsafe {
            command.pre_exec(install_child_restrictions);
        }
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "host cannot install the required child restrictions",
            ))
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "isolated VM processes require x86_64 Linux",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn seccomp_denies_syscall_number(number: u32) -> bool {
        number & super::X32_SYSCALL_BIT != 0
            || number == libc::SYS_setsid as u32
            || number == libc::SYS_setpgid as u32
            || number == libc::SYS_unshare as u32
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn x32_syscall_numbers_are_denied_before_native_syscall_matching() {
        assert!(seccomp_denies_syscall_number(
            libc::SYS_setsid as u32 | 0x4000_0000
        ));
        assert!(seccomp_denies_syscall_number(libc::SYS_setpgid as u32));
        assert!(!seccomp_denies_syscall_number(libc::SYS_getpid as u32));
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn child_restriction_filter_installs_in_a_separate_process() {
        super::verify_child_restrictions().unwrap();
    }
}
