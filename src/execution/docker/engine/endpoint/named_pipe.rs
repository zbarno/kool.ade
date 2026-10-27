#[cfg(windows)]
mod acl;
#[cfg(windows)]
mod image;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bollard::{API_DEFAULT_VERSION, BollardRequest, Docker};
use hyper::Uri;
use hyper::rt::{Read, Write};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tower_service::Service;
use windows_sys::Win32::Foundation::HANDLE;

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

const CANONICAL_PIPE: &str = r"\\.\pipe\docker_engine";
const PIPE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// The pipe ACL, server process identity, and Authenticode chain are checked before this transport
// is used. Native Windows validation is still required to confirm those checks against supported
// Docker Desktop installer variants. Unknown ACLs, signatures, and custom install roots fail
// closed; non-Linux application execution remains disabled by the endpoint policy.
pub(super) fn connect(path: &str) -> Result<Docker, PrerequisiteFailure> {
    if !is_canonical_pipe_path(path) {
        return Err(untrusted_pipe());
    }

    let mut builder = Client::builder(TokioExecutor::new());
    builder.http1_title_case_headers(true);
    builder.pool_max_idle_per_host(0);
    let client = Arc::new(builder.build(TrustedPipeConnector));
    Docker::connect_with_custom_transport(
        move |request: BollardRequest| {
            let client = Arc::clone(&client);
            async move {
                client
                    .request(request)
                    .await
                    .map_err(bollard::errors::Error::from)
            }
        },
        Some("net.pipe://docker_engine"),
        5,
        API_DEFAULT_VERSION,
    )
    .map_err(|_| untrusted_pipe())
}

fn untrusted_pipe() -> PrerequisiteFailure {
    crate::execution::docker::discovery::PrerequisiteFailure {
        code: PrerequisiteCode::UntrustedEndpoint,
        message: "Docker Desktop named-pipe provenance could not be verified".into(),
        retryable: false,
    }
}

fn is_canonical_pipe_path(path: &str) -> bool {
    path == CANONICAL_PIPE
}

#[derive(Clone, Copy, Debug)]
struct TrustedPipeConnector;

impl Service<Uri> for TrustedPipeConnector {
    type Response = TrustedPipe;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, destination: Uri) -> Self::Future {
        Box::pin(async move {
            if !is_canonical_pipe_uri(&destination) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Docker named-pipe URI is not canonical",
                ));
            }

            open_trusted_pipe().await
        })
    }
}

fn is_canonical_pipe_uri(destination: &Uri) -> bool {
    destination.scheme_str() == Some("net.pipe")
        && destination
            .authority()
            .is_some_and(|authority| authority.as_str() == "docker_engine")
}

async fn open_trusted_pipe() -> io::Result<TrustedPipe> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Pipes::{
        GetNamedPipeServerProcessId, GetNamedPipeServerSessionId,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    let wide_path: Vec<u16> = std::ffi::OsStr::new(CANONICAL_PIPE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let deadline = Instant::now() + PIPE_CONNECT_TIMEOUT;
    let handle: HANDLE = loop {
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ
                    | GENERIC_WRITE
                    | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            break handle;
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) || Instant::now() >= deadline {
            return Err(error);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    // SAFETY: CreateFileW returned an owned, overlapped handle for this pipe. Tokio takes
    // ownership and registers that exact handle with the active runtime.
    let client = unsafe { NamedPipeClient::from_raw_handle(handle as _) }?;
    let raw_handle = std::os::windows::io::AsRawHandle::as_raw_handle(&client) as HANDLE;
    let mut server_pid = 0;
    if unsafe { GetNamedPipeServerProcessId(raw_handle, &mut server_pid) } == 0
        || server_pid == 0
        || server_pid == unsafe { GetCurrentProcessId() }
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Docker named-pipe server process could not be verified",
        ));
    }
    // Pin the server process object immediately after reading its PID; keep it alive through the
    // ACL, image, signature, and final peer checks below.
    let server_process = image::ServerProcess::open(server_pid)?;

    let mut server_session = 0;
    let mut current_session = 0;
    if unsafe { GetNamedPipeServerSessionId(raw_handle, &mut server_session) } == 0
        || unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut current_session) } == 0
        || server_session != current_session
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Docker named-pipe server session could not be verified",
        ));
    }
    acl::validate_handle(raw_handle)?;
    let verified_server_process = server_process.validate()?;

    let mut verified_server_pid = 0;
    if unsafe { GetNamedPipeServerProcessId(raw_handle, &mut verified_server_pid) } == 0
        || verified_server_pid != server_pid
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Docker named-pipe server changed during provenance validation",
        ));
    }
    let mut verified_server_session = 0;
    if unsafe { GetNamedPipeServerSessionId(raw_handle, &mut verified_server_session) } == 0
        || verified_server_session != server_session
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Docker named-pipe server session changed during provenance validation",
        ));
    }
    verified_server_process.ensure_running()?;

    Ok(TrustedPipe(Box::pin(TokioIo::new(client))))
}

struct TrustedPipe(Pin<Box<TokioIo<NamedPipeClient>>>);

impl Connection for TrustedPipe {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl Read for TrustedPipe {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        self.get_mut().0.as_mut().poll_read(context, buffer)
    }
}

impl Write for TrustedPipe {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().0.as_mut().poll_write(context, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().0.as_mut().poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().0.as_mut().poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CANONICAL_PIPE, Uri, image::is_trusted_image_identity, is_canonical_pipe_path,
        is_canonical_pipe_uri,
    };

    #[test]
    fn rejects_noncanonical_pipe_names() {
        for path in [
            r"\\.\pipe\docker_engine2",
            r"\\localhost\pipe\docker_engine",
            r"\\.\pipe\Docker_Engine",
        ] {
            assert!(!is_canonical_pipe_path(path));
        }
        assert!(is_canonical_pipe_path(CANONICAL_PIPE));
    }

    #[test]
    fn rejects_noncanonical_pipe_uris() {
        for uri in [
            "net.pipe://docker_engine2/v1.41/info",
            "net.pipe://docker_engine:2375/v1.41/info",
            "http://docker_engine/v1.41/info",
            "net.pipe://localhost/pipe/docker_engine",
        ] {
            assert!(!is_canonical_pipe_uri(&uri.parse::<Uri>().unwrap()));
        }
        assert!(is_canonical_pipe_uri(
            &"net.pipe://docker_engine/v1.41/info"
                .parse::<Uri>()
                .unwrap()
        ));
    }

    #[test]
    fn rejects_untrusted_server_executable_or_publisher() {
        assert!(!is_trusted_image_identity(
            r"C:\Program Files\Docker\Docker\resources\docker.exe",
            "Docker Inc."
        ));
        assert!(!is_trusted_image_identity(
            r"C:\Program Files\Docker\Docker\resources\com.docker.backend.exe",
            "A different publisher"
        ));
    }
}
