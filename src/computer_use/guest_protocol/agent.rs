mod action;
mod bootstrap;
mod platform;
mod protocol;
mod replay;
mod staged_task;
mod x11;
use platform::{guest_capabilities, readable, write_exit_notice};
use protocol::{envelope, write_outcome};
use replay::{action_digest, request_id_for_action};

use super::{
    AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestEnvelopeContextV1, GuestMessageV1,
    GuestProtocolErrorV1, GuestSenderV1,
    auth::{GuestHandshakeCompleteV1, guest_handshake},
    frame::{read_frame, write_frame},
    types::{canonical_uuid, required_capability},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

const BOOT_CONFIG: &str = "/run/koolade-computer-use/boot.json";
const GUEST_PORT: &str = "/dev/virtio-ports/org.koolade.computer-control.v1";
const MAX_REPLAY_RESULTS: usize = 64;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReplayEntryV1 {
    action_sha256: String,
    outcome: super::GuestActionOutcomeV1,
}

#[derive(Default)]
struct AgentReplayState {
    completed: BTreeMap<u64, (String, ReplayEntryV1)>,
    highest_sequence: u64,
    request_digests: HashMap<String, String>,
    latest_observation: Option<super::GuestObservationV1>,
}

pub fn run() -> Result<(), GuestProtocolErrorV1> {
    let config = bootstrap::read_boot_config(Path::new(BOOT_CONFIG))?;
    let mut replay = AgentReplayState {
        highest_sequence: config.initial_action_sequence,
        ..AgentReplayState::default()
    };
    loop {
        let capabilities = guest_capabilities(&config);
        if !capabilities.screen_capture {
            thread::sleep(Duration::from_millis(250));
            continue;
        }
        match OpenOptions::new().read(true).write(true).open(GUEST_PORT) {
            Ok(mut port) => match guest_handshake(&mut port, &config, capabilities.clone()) {
                Ok(handshake) => {
                    let result = serve_connection(&mut port, &config, handshake, &mut replay);
                    if !matches!(result, Err(GuestProtocolErrorV1::Io)) {
                        eprintln!("computer-use companion connection ended");
                    }
                }
                Err(_) => eprintln!("computer-use companion authentication failed"),
            },
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    }
}

pub fn bootstrap() -> Result<(), GuestProtocolErrorV1> {
    bootstrap::install_boot_config()
}

fn serve_connection(
    port: &mut (impl Read + Write + std::os::fd::AsRawFd),
    config: &GuestBootConfigV1,
    handshake: GuestHandshakeCompleteV1,
    replay: &mut AgentReplayState,
) -> Result<(), GuestProtocolErrorV1> {
    let mut host_sequence = handshake.host_sequence;
    let mut guest_sequence = handshake.guest_sequence;
    let started_at = Instant::now();
    loop {
        let ready = readable(port.as_raw_fd(), HEARTBEAT_INTERVAL)?;
        if !ready {
            let heartbeat = envelope(
                config,
                GuestEnvelopeContextV1 {
                    sender: GuestSenderV1::Guest,
                    sequence: guest_sequence,
                    request_id: handshake.request_id.clone(),
                    action_id: handshake.action_id.clone(),
                    deadline_unix_ms: config.expires_at_unix_ms,
                    capability_id: "computer_use.channel.v1".to_owned(),
                },
                GuestMessageV1::Heartbeat {
                    boot_id: config.boot_id.clone(),
                    uptime_millis: started_at.elapsed().as_millis().min(u64::MAX as u128) as u64,
                },
            );
            write_frame(
                port,
                &AuthenticatedGuestEnvelopeV1::sign(heartbeat, handshake.channel_key.expose())?,
            )?;
            guest_sequence = guest_sequence.saturating_add(1);
            continue;
        }
        let incoming: AuthenticatedGuestEnvelopeV1 = read_frame(port)?;
        incoming.verify(handshake.channel_key.expose())?;
        let host = &incoming.envelope;
        if host.sender != GuestSenderV1::Host
            || host.protocol_version != super::types::GUEST_PROTOCOL_VERSION_V1
            || host.session_id != config.session_id
            || host.worker_id != config.worker_id
            || host.lease_generation != config.lease_generation
            || host.sequence != host_sequence
            || host.deadline_unix_ms == 0
            || host.deadline_unix_ms > config.expires_at_unix_ms
        {
            return Err(GuestProtocolErrorV1::Replay);
        }
        host_sequence = host_sequence.saturating_add(1);
        let action = match &host.message {
            GuestMessageV1::ActionRequest { action } => action,
            GuestMessageV1::StageTaskApplicationStart { action } => {
                if host.capability_id != required_capability(action)
                    || host.request_id != request_id_for_action(config, action)
                    || host.action_id != action.idempotency_key
                    || action.session_id != config.session_id
                    || action.lease_generation != config.lease_generation
                    || action.actor != crate::computer_use::ComputerActionActorV1::Human
                    || action.action_sequence <= replay.highest_sequence
                    || replay.completed.contains_key(&action.action_sequence)
                    || !canonical_uuid(&action.idempotency_key)
                {
                    return Err(GuestProtocolErrorV1::WrongSession);
                }
                (host_sequence, guest_sequence) = staged_task::receive_transfer(
                    port,
                    config,
                    &handshake,
                    host,
                    action,
                    host_sequence,
                    guest_sequence,
                )?;
                continue;
            }
            _ => return Err(GuestProtocolErrorV1::Malformed),
        };
        if host.capability_id != required_capability(action)
            || host.request_id != request_id_for_action(config, action)
            || host.action_id != action.idempotency_key
            || action.session_id != config.session_id
            || action.lease_generation != config.lease_generation
            || !canonical_uuid(&action.idempotency_key)
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        let action_sha256 = action_digest(action)?;
        if let Some((previous_request_id, entry)) = replay.completed.get(&action.action_sequence) {
            if previous_request_id != &host.request_id || entry.action_sha256 != action_sha256 {
                return Err(GuestProtocolErrorV1::Replay);
            }
            guest_sequence = write_outcome(
                port,
                config,
                &handshake,
                guest_sequence,
                host,
                &entry.outcome,
            )?;
            continue;
        }
        if let Some(previous) = replay.request_digests.get(&host.request_id)
            && previous != &action_sha256
        {
            return Err(GuestProtocolErrorV1::Replay);
        }
        if action.action_sequence <= replay.highest_sequence {
            return Err(GuestProtocolErrorV1::SequenceGap);
        }
        let outcome = match action::execute_action(action, config, replay) {
            Ok(outcome) => outcome,
            Err(error) => {
                write_exit_notice(port, config, &handshake, guest_sequence, host, error)?;
                return Err(error);
            }
        };
        replay.highest_sequence = action.action_sequence;
        replay
            .request_digests
            .insert(host.request_id.clone(), action_sha256.clone());
        replay.completed.insert(
            action.action_sequence,
            (
                host.request_id.clone(),
                ReplayEntryV1 {
                    action_sha256,
                    outcome: outcome.clone(),
                },
            ),
        );
        while replay.completed.len() > MAX_REPLAY_RESULTS {
            let Some(first) = replay.completed.keys().next().copied() else {
                break;
            };
            replay.completed.remove(&first);
        }
        guest_sequence = write_outcome(port, config, &handshake, guest_sequence, host, &outcome)?;
    }
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;
