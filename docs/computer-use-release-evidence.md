# Computer Use cross-tier release evidence

**Status recorded:** 2026-11-12. **Release state:** not certified. Issue
[#392](https://github.com/zbarno/kool.ade/issues/392) and epic
[#375](https://github.com/zbarno/kool.ade/issues/375) remain open. A merged
component issue or a green synthetic test does not establish a released
Computer Use capability.

This is the cross-tier evidence index for #392. It distinguishes merged
implementation, automated checks, real-host evidence, and release enablement.
Only evidence linked below has been observed; pending or missing evidence is
not a pass.

## Evidence matrix

| Capability | Implementation and automated evidence | Real-host / release evidence | Current gate and remaining proof |
| --- | --- | --- | --- |
| R1 offline disposable VM | Signed generation 4 guest image is catalog-pinned. The fw_cfg sysfs zero-size bug was fixed by [PR #593](https://github.com/zbarno/kool.ade/pull/593), and two independently configured worker fixtures were added in [PR #595](https://github.com/zbarno/kool.ade/pull/595). Both PRs are merged; their hosted checks passed. | The local Linux x86_64 host passed the read-only preflight inside a transient `systemd-run --user --scope --property=Delegate=yes` scope. The prior real QEMU run against generation 4 failed before `ClientHello`; the guest helper fix is not in that signed image. | Main candidate build and attestation [43189760047](https://github.com/zbarno/kool.ade/actions/runs/43189760047) passed. Promotion run [43192209496](https://github.com/zbarno/kool.ade/actions/runs/43192209496) passed and created draft generation 5 plus catalog review [PR #596](https://github.com/zbarno/kool.ade/pull/596); its hosted checks and review are pending. No online `computer-use-kvm` Actions runner is registered. Run production real-KVM acceptance after the reviewed catalog change merges and the signed release is published; add guest/host isolation negatives and repeated cleanup evidence before enabling R1. |
| Host readiness and novice setup (#378) | The readiness and setup implementation is merged; preflight checks Linux architecture, QEMU/KVM access, root-owned QEMU binaries, and delegated cgroup v2 controllers. | The current host passed preflight only when run from a delegated transient user scope. | Attach a successful real guest boot/action/cleanup run to #392. A preflight alone is host-prerequisite evidence, not VM acceptance. |
| Guest image and authenticated companion (#379, #380, #381) | Generation 4 is the current published signed catalog entry. PR #593 fixes reading fw_cfg sysfs content when metadata reports length zero while preserving bounded reads. Its local regression tests and hosted checks passed. | Main candidate run 43189760047 passed build, reproducibility, and provenance attestation. Promotion run 43192209496 validated and signed generation 5. Its catalog update is under review in [PR #596](https://github.com/zbarno/kool.ade/pull/596); the draft release is not published until that PR merges. No post-fix guest boot has run yet. | Review and merge the catalog promotion PR, then verify the new signed image digest, companion digest, authenticated handshake, GUI action, display stream, and cleanup on KVM. |
| GUI actions, viewer/takeover, guest apps, artifacts, and QA repair (#382, #383, #385, #389, #394) | The component issues are merged with automated contract, policy, artifact, provisioning, and UI coverage. The guest reports accessibility-tree inspection as unsupported in v1. | No successful post-fix real-VM end-to-end run is recorded. The R1 real-KVM workflow is the configured production boundary; mock and synthetic-browser tests do not prove this path. | Verify app launch, frame/input round trip, artifact isolation, seeded QA repair, live viewer/takeover/handback, and cleanup through the signed guest on the configured real-VM path. Capture viewport/accessibility results and performance timings. |
| R2 public Internet research (#386) | The restricted research network profile and proxy boundary are implemented and covered by policy/component tests. | No real-VM outbound-network, metadata, private-destination, or proxy-bypass matrix is recorded in #392. | Keep claims limited to the implemented profile until real-VM network-denial and allowed-destination evidence is attached. |
| R3 encrypted personal VM and synthetic authentication (#387) | The implementation is merged. The synthetic IdP/MFA/browser-restart job passed in candidate run 43189760047. | No personal persistent VM was accepted on a real host. | `PERSONAL_VM_R3_ENABLED` is `false`; keep personal mode unavailable until its threat-model and real-host evidence are accepted. |
| R4 sensitive external actions (#388) | The policy validator rejects `HighImpactExternalAction` until exact transaction grants exist; denial and stale-approval tests are present. | No checkout, publication, or other external side-effect flow is certified. No real money or real accounts are used in CI. | High-impact external actions remain unsupported. A future gate needs synthetic-account allow/deny, changed-detail, duplicate-execution, and uncertain-outcome evidence. |
| R5 remote VM workers (#390) | Remote worker code merged in [PR #572](https://github.com/zbarno/kool.ade/pull/572). Separate worker identity/stream fixtures merged in [PR #595](https://github.com/zbarno/kool.ade/pull/595); the tests reject wrong host fingerprints and cross-worker sessions. | These fixtures use in-process stdio streams. They are not real SSH connections or separate physical hosts. | `remote_computer_use_release_gate_open_v1()` is `false`; #390 remains open for independent real-host worker, restart, interruption, and competing-operator evidence. |
| R5 macOS and Windows host adapters (#395) | The issue remains open; no adapter is certified. | No macOS or Windows host VM evidence is recorded. | Unsupported. Do not start adapter certification until the verified Linux baseline required by #395 is accepted. |

## Reproducibility record

The last published signed catalog is generation 4, manifest digest
`sha256:d74a675415b7ec16365bd97da55c2fbc0a5b51168ea89ec8f716fca2743c66c5`,
with approved x86_64 image digest
`sha256:91fdae70e2e1a4ea756b4efbaa85de0fef0d70e6485119bf7b5a2a957b73887f`
and companion protocol version 1. Its companion digest is
`sha256:3d9643a033bb3edc4bbb41367d9b27d9b878b51b376cef670ea0aeedc0839a96`.
The source revision for that guest image is
`ee89d354cec4664b9adf5b329c58ab23b7990542`.

The generation 5 candidate is based on `7d2975712055662e49add1a1607535826f4b8287`.
Its image digest is
`sha256:38f46622092ecbbc64b10d1720dba678f696ec12d8ef89428a242a10d0845b41`,
its companion digest is
`sha256:8219429f4657625f1afc57cbddca57e197e4635dfdb3c1810c12da6cf960ca8a`,
and its signed catalog manifest digest is
`sha256:0b9b6034ea7220ef09e48d29d1b66cda2f308289e5da6aaa481705229faa1992`.
The catalog update is [PR #596](https://github.com/zbarno/kool.ade/pull/596),
and its associated GitHub release remains a draft until that PR merges.

The local preflight host was Linux x86_64, kernel
`7.2.15-76070215-generic`, an 11th Gen Intel Core i7-1195G7, with QEMU and
qemu-img `8.2.2`; `systemd-detect-virt --vm` reported `none`. `/dev/kvm` was
read/write accessible. Delegated `cpu`, `memory`, and `pids` controllers were
available from the transient delegated user scope. This was rechecked after
installing `qemu-system-x86` and `qemu-utils`; the host preflight passes. This
is prerequisite evidence only; it does not establish a boot, guest action,
isolation, or release pass. The repository currently has no online Actions runner labeled
`computer-use-kvm`.

The official real-KVM workflow is manually dispatched on protected `main` and
requires that runner label. A run with `require_kvm_host=false` is explicitly
not acceptance evidence. The owner-operated usability walkthrough is not a
closure gate; the automated real-host security and release checks above still
are.

## Release controls and gaps

- R3 personal persistent sessions remain disabled by
  `PERSONAL_VM_R3_ENABLED = false`.
- R4 high-impact external actions are rejected by policy until exact
  transaction grants exist.
- Remote VM scheduling remains disabled by
  `remote_computer_use_release_gate_open_v1() = false`.
- macOS and Windows host adapters remain unsupported while #395 is open.
- Accessibility-tree inspection is unsupported in the guest v1 agent.
- No verified post-fix real-VM run, adversarial guest/host isolation matrix,
  repeated resource/cleanup run, or accepted provisioning/screen/idle
  performance measurements are recorded yet.
- At merge time, PRs #593 and #595 were merged by `zbarno`; GitHub had no
  submitted formal review on either PR. The owner may review manually, but no
  independent GitHub review event is asserted here.

Until those gaps are addressed and the corresponding issue evidence is
updated, #392 and #375 stay open and unsupported tiers remain fail-closed.
