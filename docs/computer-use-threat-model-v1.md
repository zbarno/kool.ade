# Computer Use threat model (v1)

This is the CU-01 threat register. The guest and all content rendered inside it
are untrusted. Severity is the impact if the stated boundary fails. Test IDs
refer to deterministic contract tests unless a later release gate is named;
they do not claim real VM isolation.

| Threat | Severity | Required gate | Regression evidence |
| --- | --- | --- | --- |
| Malicious webpage/DOM injects instructions | High | Treat page, DOM, and accessibility text as data; policy authority stays in controller | `capability_restriction_only_intersects_and_lowers_authority_and_quotas`; CU16 adversarial prompt-injection E2E |
| Malicious GUI app or compromised guest companion forges state | Critical | Per-boot nonce challenge and HMAC channel; host trusts only controller-fenced events | guest protocol handshake, identity-binding, and replay tests; real virtio-serial/KVM boundary remains #392 |
| Web credential exfiltration | Critical | Offline default; isolated egress gateway; no inherited credentials; human-only secret path | `read_only_capability_cannot_include_input_and_restriction_can_revoke_all_operations`; CU10 egress/secret-canary E2E |
| Virtual network SSRF or metadata access | Critical | No guest NIC by default; separately enforced destination policy for research | `capability_restriction_only_intersects_and_lowers_authority_and_quotas`; CU10 private-address/metadata denial E2E |
| Screenshot, clipboard, or accessibility leak | High | Bounded opaque evidence refs, redaction/retention policy, no implicit clipboard bridge | `golden_wire_contracts_validate_and_round_trip_without_shape_drift`; CU09 content redaction tests |
| File transfer crosses task, guest, or user boundary | Critical | Exact identity/revision/path grants, one-use lease and approval generations, bounded private staging, digest checks, and explicit user export | `computer_use::artifacts` grant, traversal, link, digest, collision, expiry, clipboard, and revocation tests; authenticated VM bridge remains #392 |
| Replayed/replaced action or result | Critical | Session identity, idempotency key, contiguous sequence, durable host journal, immutable generations | guest journal and authenticated transport tests; real KVM boundary remains #392 |
| VNC/display stream hijack | High | Authenticated private transport; viewer attaches through controller | CU08 viewer-auth E2E |
| VM escape or malicious device exposure | Critical | Dedicated unprivileged host process, no passthrough/shared host paths, maintained QEMU boundary | CU16 real-KVM adversarial release gate; contract tests are insufficient |
| Guest-to-host clipboard path traversal | Critical | No host-mounted clipboard/file bridge; later bridge uses typed bounded artifacts | `wire_decoder_rejects_unknown_permissions_credentials_paths_and_versions`; CU09 traversal tests |
| Malicious or rolled-back snapshot | High | Immutable verified base, per-session overlay, digest/approval recheck | CU03 digest/tamper and overlay isolation tests |
| Remote worker impersonation | Critical | V1 targets local KVM only; future remote worker needs separate authenticated identity binding | CU01-T-local-target-only; CU14 handshake tests |
| Approval spoofing or stale purchase replay | Critical | Controller-only approval generation; v1 has no high-impact transaction grant; uncertain effects are never retried | `event_stream_fences_sequences_actors_and_uncertain_external_effects`; CU12 exact-action approval/replay E2E |
| Duplicate billing or external side effect after timeout | Critical | Idempotency and explicit uncertain outcome; no automatic retry after ambiguous completion | `event_stream_fences_sequences_actors_and_uncertain_external_effects`; CU12 failure-injection E2E |
| VM isolation mistaken for logged-in account safety | Critical | Docs/UI must state account risk; persistent sessions separately gated and encrypted | CU01-T-no-implicit-persistence; CU11 identity/takeover security review |
| Model or guest mints/renews authority | Critical | Controller-only policy and lease issuance; strict schemas; generation fence | `controller_attestation_binds_identity_policy_lease_target_and_image`; `wire_decoder_rejects_unknown_permissions_credentials_paths_and_versions`; `lease_renewal_must_advance_identity_fence_and_expiry`; CU05/07 authority tests |
| Guest changes task repository or publishes output | Critical | Controller owns source of truth; guest returns evidence only; controller applies under task claim | `golden_wire_contracts_validate_and_round_trip_without_shape_drift`; CU13 claim-fenced integration |

## Explicit limitations

The guest companion authenticates the local channel and the host journal
prevents automatic replay after uncertain outcomes. This does not prove real
VM isolation, disk encryption, viewer authentication, secure secret entry,
or content redaction. Those require owning components and end-to-end tests in
the linked issues. Logged-in accounts remain exposed to whatever a guest application
can do inside that account, even when the VM boundary works as designed.
