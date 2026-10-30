use std::path::Path;

/// Atomic publication without replacing an occupied destination.
pub(super) fn directory(from: &Path, to: &Path) -> anyhow::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        anyhow::ensure!(
            result == 0,
            "could not publish project folder: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
        let mut source: Vec<u16> = from.as_os_str().encode_wide().collect();
        let mut destination: Vec<u16> = to.as_os_str().encode_wide().collect();
        anyhow::ensure!(
            !source.contains(&0) && !destination.contains(&0),
            "invalid project location"
        );
        source.push(0);
        destination.push(0);
        // Deliberately omit REPLACE_EXISTING and COPY_ALLOWED. Staging and
        // destination share a parent, and an occupied destination must fail.
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        anyhow::ensure!(
            result != 0,
            "could not publish project folder: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (from, to);
        anyhow::bail!("safe project-folder publication is not available on this platform yet")
    }
}
