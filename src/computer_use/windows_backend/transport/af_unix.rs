use std::{
    io::{self, Read, Write},
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, FIONBIO, INVALID_SOCKET, SIO_AF_UNIX_GETPEERPID, SOCK_STREAM, SOCKADDR, SOCKADDR_UN,
    SOCKET, SOCKET_ERROR, WSADATA, WSAEWOULDBLOCK, WSAGetLastError, WSAIoctl, WSAStartup, accept,
    bind, closesocket, ioctlsocket, listen, recv, send, setsockopt, socket,
};

use crate::computer_use::backend::GuestChannelIo;

use super::super::process::WindowsVmProcessJob;
use super::channel_from_verified_io;

const WINSOCK_VERSION_2_2: u16 = 0x0202;
const SOL_SOCKET: i32 = 0xffff;
const SO_RCVTIMEO: i32 = 0x1006;
const SO_SNDTIMEO: i32 = 0x1005;
const MAX_ACCEPT_WAIT: Duration = Duration::from_secs(120);

static WINSOCK_STATUS: OnceLock<Result<(), i32>> = OnceLock::new();

pub(crate) struct WindowsAfUnixListener {
    socket: SocketHandle,
}

struct SocketHandle(SOCKET);

impl Drop for SocketHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a valid Winsock socket.
        unsafe { closesocket(self.0) };
    }
}

impl WindowsAfUnixListener {
    pub(in crate::computer_use::windows_backend) fn bind(path: &Path) -> io::Result<Self> {
        ensure_winsock()?;
        let path = path
            .to_str()
            .ok_or_else(|| invalid_input("AF_UNIX path is not valid UTF-8"))?;
        if !Path::new(path).is_absolute() || path.as_bytes().len() >= 108 || path.contains('\0') {
            return Err(invalid_input("AF_UNIX path is not an absolute short path"));
        }

        // SAFETY: Winsock is initialized and the requested family/type are supported.
        let socket = unsafe { socket(AF_UNIX.into(), SOCK_STREAM, 0) };
        if socket == INVALID_SOCKET {
            return Err(last_winsock_error());
        }
        let socket = SocketHandle(socket);
        let mut address = SOCKADDR_UN::default();
        address.sun_family = AF_UNIX;
        for (slot, byte) in address.sun_path.iter_mut().zip(path.as_bytes()) {
            *slot = *byte as i8;
        }
        // SAFETY: `address` has the documented SOCKADDR_UN layout; path length
        // was checked against the fixed sun_path field and the field is zero-filled.
        if unsafe {
            bind(
                socket.0,
                (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
                std::mem::size_of::<SOCKADDR_UN>() as i32,
            )
        } == SOCKET_ERROR
        {
            return Err(last_winsock_error());
        }
        // SAFETY: this socket has been bound and is now a single-client listener.
        if unsafe { listen(socket.0, 1) } == SOCKET_ERROR {
            return Err(last_winsock_error());
        }
        let mut nonblocking = 1_u32;
        // SAFETY: `nonblocking` is a writable FIONBIO input value.
        if unsafe { ioctlsocket(socket.0, FIONBIO, &mut nonblocking) } == SOCKET_ERROR {
            return Err(last_winsock_error());
        }
        Ok(Self { socket })
    }

    pub(crate) fn accept_guest_channel(
        &self,
        session_id: String,
        lease_generation: u64,
        qemu: &mut WindowsVmProcessJob,
        timeout: Duration,
    ) -> io::Result<crate::computer_use::ComputerGuestChannelV1> {
        if timeout.is_zero() || timeout > MAX_ACCEPT_WAIT {
            return Err(invalid_input("AF_UNIX accept timeout is outside its bound"));
        }
        let deadline = Instant::now() + timeout;
        let expected_pid = qemu.id();
        if expected_pid == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "WHPX QEMU process identity is unavailable",
            ));
        }
        loop {
            if qemu.try_wait()?.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "WHPX QEMU exited before guest transport connected",
                ));
            }
            // SAFETY: the listener is valid; null address outputs accept any peer.
            let accepted =
                unsafe { accept(self.socket.0, std::ptr::null_mut(), std::ptr::null_mut()) };
            if accepted != INVALID_SOCKET {
                let stream = SocketHandle(accepted);
                verify_peer_pid(stream.0, expected_pid)?;
                if qemu.try_wait()?.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "AF_UNIX QEMU process exited during peer verification",
                    ));
                }
                let mut blocking = 0_u32;
                // SAFETY: `blocking` requests a blocking connected stream socket.
                if unsafe { ioctlsocket(stream.0, FIONBIO, &mut blocking) } == SOCKET_ERROR {
                    return Err(last_winsock_error());
                }
                return Ok(channel_from_verified_io(
                    session_id,
                    lease_generation,
                    WindowsAfUnixStream(stream),
                ));
            }
            let error = last_winsock_error();
            if error.raw_os_error() != Some(WSAEWOULDBLOCK) {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "WHPX guest transport did not connect before the deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct WindowsAfUnixStream(SocketHandle);

impl Read for WindowsAfUnixStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let len = buffer.len().min(i32::MAX as usize) as i32;
        // SAFETY: the writable buffer is valid for `len` bytes and the socket is owned.
        let result = unsafe { recv(self.0.0, buffer.as_mut_ptr(), len, 0) };
        if result == SOCKET_ERROR {
            Err(last_winsock_error())
        } else {
            Ok(result as usize)
        }
    }
}

impl Write for WindowsAfUnixStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let len = buffer.len().min(i32::MAX as usize) as i32;
        // SAFETY: the readable buffer is valid for `len` bytes and the socket is owned.
        let result = unsafe { send(self.0.0, buffer.as_ptr(), len, 0) };
        if result == SOCKET_ERROR {
            Err(last_winsock_error())
        } else {
            Ok(result as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl GuestChannelIo for WindowsAfUnixStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        set_socket_timeout(self.0.0, SO_RCVTIMEO, timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        set_socket_timeout(self.0.0, SO_SNDTIMEO, timeout)
    }
}

fn set_socket_timeout(socket: SOCKET, option: i32, timeout: Option<Duration>) -> io::Result<()> {
    let milliseconds = timeout
        .map(|duration| duration.as_millis().clamp(1, u32::MAX as u128) as u32)
        .unwrap_or(0);
    // SAFETY: Winsock reads one DWORD from the valid stack value.
    if unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            option,
            (&milliseconds as *const u32).cast(),
            std::mem::size_of::<u32>() as i32,
        )
    } == SOCKET_ERROR
    {
        Err(last_winsock_error())
    } else {
        Ok(())
    }
}

fn query_peer_pid(socket: SOCKET) -> io::Result<u32> {
    let mut process_id = 0_u32;
    let mut bytes_returned = 0_u32;
    // SAFETY: the connected AF_UNIX socket is valid and both output pointers
    // refer to writable values of the documented sizes.
    let status = unsafe {
        WSAIoctl(
            socket,
            SIO_AF_UNIX_GETPEERPID,
            std::ptr::null(),
            0,
            (&mut process_id as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
            None,
        )
    };
    if status == SOCKET_ERROR {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Windows AF_UNIX peer PID query failed: {}",
                last_winsock_error()
            ),
        ));
    }
    if process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows AF_UNIX peer PID is unavailable",
        ));
    }
    let _ = bytes_returned; // Some Windows builds report zero while returning the PID.
    Ok(process_id)
}

fn verify_peer_pid(socket: SOCKET, expected_pid: u32) -> io::Result<()> {
    let peer_pid = query_peer_pid(socket)?;
    if peer_pid != expected_pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "AF_UNIX guest transport peer is not the active QEMU process",
        ));
    }
    Ok(())
}

fn ensure_winsock() -> io::Result<()> {
    let status = *WINSOCK_STATUS.get_or_init(|| {
        let mut data = WSADATA::default();
        // SAFETY: `data` is the writable output structure required by WSAStartup.
        let result = unsafe { WSAStartup(WINSOCK_VERSION_2_2, &mut data) };
        if result == 0 { Ok(()) } else { Err(result) }
    });
    status.map_err(io::Error::from_raw_os_error)
}

fn last_winsock_error() -> io::Error {
    // SAFETY: retrieves the calling thread's most recent Winsock error code.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
#[path = "af_unix_tests.rs"]
mod tests;
