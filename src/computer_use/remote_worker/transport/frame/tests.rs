use super::*;
use crate::computer_use::remote_worker::ComputerRemoteWorkerRequestV1;

#[test]
fn length_prefixed_remote_frames_round_trip_and_reject_ambiguous_lengths() {
    let request = ComputerRemoteWorkerRequestV1 {
        protocol_version: 1,
        worker_id: "worker-east".into(),
        ssh_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        lease_id: "00000000-0000-4000-8000-000000000002".into(),
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        transport_sequence: 1,
        request_id: "00000000-0000-4000-8000-000000000003".into(),
        body: crate::computer_use::remote_worker::ComputerRemoteWorkerRequestBodyV1::Operation {
            operation:
                crate::computer_use::remote_worker::ComputerRemoteWorkerOperationV1::Release {
                    handle: crate::computer_use::ComputerMachineHandleV1::from_remote(
                        "00000000-0000-4000-8000-000000000001".into(),
                        "00000000-0000-4000-8000-000000000002".into(),
                        1,
                        "00000000-0000-4000-8000-000000000004".into(),
                    ),
                },
        },
    };
    let frame = encode_remote_computer_worker_frame_v1(&request).unwrap();
    assert_eq!(
        decode_remote_computer_worker_frame_v1::<ComputerRemoteWorkerRequestV1>(&frame).unwrap(),
        request
    );
    assert!(
        decode_remote_computer_worker_frame_v1::<ComputerRemoteWorkerRequestV1>(&frame[..5])
            .is_err()
    );
    let mut trailing = frame.clone();
    trailing.push(0);
    assert!(
        decode_remote_computer_worker_frame_v1::<ComputerRemoteWorkerRequestV1>(&trailing).is_err()
    );
    assert!(
        decode_remote_computer_worker_frame_v1::<ComputerRemoteWorkerRequestV1>(
            &u32::MAX.to_be_bytes()
        )
        .is_err()
    );
}

#[test]
fn remote_frame_encoder_enforces_the_payload_bound() {
    let bytes = vec![b'x'; MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1 + 1];
    assert!(encode_remote_computer_worker_frame_v1(&bytes).is_err());
}
