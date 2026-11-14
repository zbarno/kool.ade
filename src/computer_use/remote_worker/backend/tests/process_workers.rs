use super::independent_workers::{session_for_worker, worker_config, worker_request};
use super::*;
use std::{
    os::unix::net::{UnixListener, UnixStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const PROCESS_FIXTURE_ENV: &str = "KOOLADE_TEST_REMOTE_WORKER_PROCESS_FIXTURE";
const PROCESS_FIXTURE_CONFIG: &str = "KOOLADE_TEST_REMOTE_WORKER_CONFIG";
const PROCESS_FIXTURE_SOCKET: &str = "KOOLADE_TEST_REMOTE_WORKER_SOCKET";
const PROCESS_FIXTURE_EXPECT_REJECTION: &str = "KOOLADE_TEST_REMOTE_WORKER_EXPECT_REJECTION";

struct WorkerProcess(Child);

impl WorkerProcess {
    fn wait_success(&mut self) {
        assert!(self.0.wait().unwrap().success());
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn spawn_worker_process(
    config: &ComputerRemoteWorkerHostConfigV1,
    root: &std::path::Path,
    name: &str,
    expect_rejection: bool,
) -> (ComputerRemoteWorkerClientV1, WorkerProcess) {
    let config_path = root.join(format!("{name}.json"));
    let socket_path = root.join(format!("{name}.sock"));
    std::fs::write(&config_path, serde_json::to_vec(config).unwrap()).unwrap();

    let child = Command::new(std::env::current_exe().unwrap())
        .args(["worker_fixture_process_entrypoint", "--nocapture"])
        .env(PROCESS_FIXTURE_ENV, "1")
        .env(PROCESS_FIXTURE_CONFIG, &config_path)
        .env(PROCESS_FIXTURE_SOCKET, &socket_path)
        .env(
            PROCESS_FIXTURE_EXPECT_REJECTION,
            if expect_rejection { "1" } else { "0" },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = WorkerProcess(child);

    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match UnixStream::connect(&socket_path) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => {
                if let Some(status) = child.0.try_wait().unwrap() {
                    panic!("worker fixture exited before accepting a stream: {status}");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("worker fixture did not accept a stream: {error}"),
        }
    };

    let identity = VerifiedComputerRemoteWorkerIdentityV1::from_host_config(config);
    (
        ComputerRemoteWorkerClientV1::from_test_stream(stream, identity, 1),
        child,
    )
}

/// Child entrypoint for process-isolated protocol tests. The Unix socket keeps
/// Rust's test-harness output separate from the framed worker stream.
#[test]
fn worker_fixture_process_entrypoint() {
    if std::env::var(PROCESS_FIXTURE_ENV).as_deref() != Ok("1") {
        return;
    }
    let config_path = std::env::var_os(PROCESS_FIXTURE_CONFIG).unwrap();
    let socket_path = std::env::var_os(PROCESS_FIXTURE_SOCKET).unwrap();
    let config: ComputerRemoteWorkerHostConfigV1 =
        serde_json::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
    let listener = UnixListener::bind(socket_path).unwrap();
    let (stream, _) = listener.accept().unwrap();
    let result = serve_computer_remote_worker_stdio_v1(
        stream.try_clone().unwrap(),
        stream,
        SharedCheckpoint::default(),
        FakeRuntime {
            config: config.clone(),
            guest: Arc::new(Mutex::new(FakeGuest::default())),
        },
        &config,
    );
    if std::env::var(PROCESS_FIXTURE_EXPECT_REJECTION).as_deref() == Ok("1") {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    } else {
        result.unwrap();
    }
}

/// Two worker endpoints run in separate OS processes with distinct private
/// configuration and keys. This exercises process-boundary framing and
/// independent session identities. It is not real SSH or separate-host
/// release evidence; those claims still require #392 evidence.
#[test]
fn process_isolated_worker_fixtures_keep_sessions_and_identities_separate() {
    let key = signing_key();
    let east_config = worker_config(
        &key,
        "worker-east-process",
        "00000000-0000-4000-8000-000000000221",
        'C',
        13,
        "00000000-0000-4000-8000-000000000231",
    );
    let west_config = worker_config(
        &key,
        "worker-west-process",
        "00000000-0000-4000-8000-000000000222",
        'D',
        14,
        "00000000-0000-4000-8000-000000000232",
    );
    let east_session = session_for_worker(&key, &east_config, 13);
    let west_session = session_for_worker(&key, &west_config, 14);
    let root = tempfile::tempdir().unwrap();
    let (mut east_client, mut east_process) =
        spawn_worker_process(&east_config, root.path(), "east", false);
    let (mut west_client, mut west_process) =
        spawn_worker_process(&west_config, root.path(), "west", true);

    for (client, config, session) in [
        (&mut east_client, &east_config, &east_session),
        (&mut west_client, &west_config, &west_session),
    ] {
        assert!(matches!(
            client
                .request(&worker_request(
                    session,
                    &config.worker_id,
                    &config.ssh_host_key_fingerprint,
                    1,
                    Body::Start {
                        session: session.clone(),
                    },
                ))
                .unwrap()
                .result,
            ResultValue::Prepared { .. }
        ));
    }

    let crossed_session = worker_request(
        &east_session,
        &west_config.worker_id,
        &west_config.ssh_host_key_fingerprint,
        2,
        Body::Start {
            session: east_session.clone(),
        },
    );
    assert!(west_client.request(&crossed_session).is_err());

    drop(east_client);
    drop(west_client);
    east_process.wait_success();
    west_process.wait_success();
}
