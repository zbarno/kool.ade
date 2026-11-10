use std::{
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    time::Duration,
};

use super::protocol::{ReplyV1, RequestV1, SOCKET, read_message, write_message};

pub(super) fn run() -> Result<(), &'static str> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("guest app helper must run as root");
    }
    super::execute::begin_guest_helper_boot()?;
    let directory = Path::new("/run/koolade-app-helper");
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|_| "helper directory unavailable")?;
    if !metadata.file_type().is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err("helper directory is unsafe");
    }
    let socket_path = Path::new(SOCKET);
    if socket_path.exists() {
        let metadata =
            std::fs::symlink_metadata(socket_path).map_err(|_| "helper socket unavailable")?;
        if !metadata.file_type().is_socket() || metadata.uid() != 0 {
            return Err("existing helper socket is unsafe");
        }
        std::fs::remove_file(socket_path).map_err(|_| "stale helper socket cannot be removed")?;
    }
    let listener =
        UnixListener::bind(socket_path).map_err(|_| "helper socket cannot be created")?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660))
        .map_err(|_| "helper socket permissions cannot be set")?;
    let group = unsafe { libc::getgrnam(c"koolade-agent".as_ptr()) };
    let socket_c = std::ffi::CString::new(socket_path.as_os_str().as_bytes())
        .map_err(|_| "helper socket path is invalid")?;
    if group.is_null() || unsafe { libc::chown(socket_c.as_ptr(), 0, (*group).gr_gid) } != 0 {
        return Err("helper socket group cannot be assigned");
    }
    for connection in listener.incoming() {
        let Ok(mut stream) = connection else { continue };
        if !authorized_peer(&stream) {
            continue;
        }
        if stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(60))))
            .is_err()
        {
            continue;
        }
        let mut pending_stage = None;
        while let Ok(request) = read_message::<_, RequestV1>(&mut stream) {
            let (reply, keep_open) = match request {
                RequestV1::Launch { app_id } => {
                    (super::execute::launch_as_desktop_user(&app_id), false)
                }
                RequestV1::Manage { action } => {
                    (super::execute::manage(&action, &mut stream), false)
                }
                RequestV1::Status => (super::execute::status(), false),
                RequestV1::StageTaskBegin { action } => {
                    if pending_stage.is_some() {
                        (ReplyV1::Failed, false)
                    } else {
                        match super::execute::begin_task_stage(&action) {
                            Ok(stage) => {
                                pending_stage = Some(stage);
                                (ReplyV1::Completed, true)
                            }
                            Err(reply) => (reply, false),
                        }
                    }
                }
                RequestV1::StageTaskChunk {
                    artifact_index,
                    offset,
                    bytes,
                } => match pending_stage.as_mut() {
                    Some(stage) => (
                        super::execute::write_task_stage_chunk(
                            stage,
                            artifact_index,
                            offset,
                            &bytes,
                        ),
                        true,
                    ),
                    None => (ReplyV1::Failed, false),
                },
                RequestV1::StageTaskFinish => match pending_stage.take() {
                    Some(stage) => (super::execute::finish_task_stage(stage), false),
                    None => (ReplyV1::Failed, false),
                },
                RequestV1::LaunchStagedTask { action } => {
                    (super::execute::launch_staged_task(&action), false)
                }
            };
            if write_message(&mut stream, &reply).is_err() || !keep_open {
                break;
            }
        }
    }
    Ok(())
}

fn authorized_peer(stream: &UnixStream) -> bool {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || length as usize != std::mem::size_of::<libc::ucred>()
    {
        return false;
    }
    let agent = unsafe { libc::getpwnam(c"koolade-agent".as_ptr()) };
    let desktop = unsafe { libc::getpwnam(c"koolade".as_ptr()) };
    !agent.is_null()
        && !desktop.is_null()
        && peer_uid_is_agent(credentials.uid, unsafe { (*agent).pw_uid }, unsafe {
            (*desktop).pw_uid
        })
}

fn peer_uid_is_agent(
    peer_uid: libc::uid_t,
    agent_uid: libc::uid_t,
    desktop_uid: libc::uid_t,
) -> bool {
    agent_uid != desktop_uid && peer_uid == agent_uid
}

use std::os::fd::AsRawFd;

#[cfg(test)]
mod tests {
    use super::peer_uid_is_agent;

    #[test]
    fn desktop_same_uid_cannot_impersonate_the_authenticated_agent() {
        let desktop_uid = 1000;
        let agent_uid = 995;
        assert!(!peer_uid_is_agent(desktop_uid, agent_uid, desktop_uid));
        assert!(peer_uid_is_agent(agent_uid, agent_uid, desktop_uid));
        assert!(!peer_uid_is_agent(agent_uid, agent_uid, agent_uid));
    }
}
