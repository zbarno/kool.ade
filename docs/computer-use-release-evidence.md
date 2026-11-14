# Computer Use cross-tier release evidence

**Status recorded:** 2026-11-14. **Release state:** not certified. Issue
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
| R1 offline disposable VM | Signed generation 19 is catalog-pinned after [PR #628](https://github.com/zbarno/kool.ade/pull/628) merged. The bounded app-helper cgroup fix is in [PR #627](https://github.com/zbarno/kool.ade/pull/627). Both PRs and their required hosted checks passed. | The local Linux x86_64 host passed production QEMU/KVM boot, authenticated Chromium launch, guest/display capture, mouse and keyboard input, lifecycle handoff fencing, and VM release against the published generation 19 image. The input/handoff acceptance is in [PR #630](https://github.com/zbarno/kool.ade/pull/630), pending hosted checks at this update. | Candidate build and attestations [43557367751](https://github.com/zbarno/kool.ade/actions/runs/43557367751), signed catalog promotion [43558652176](https://github.com/zbarno/kool.ade/actions/runs/43558652176), catalog PR [#628](https://github.com/zbarno/kool.ade/pull/628), and published release [43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152) passed. This positive path does not establish guest/host isolation negatives, file bridge, seeded QA repair, live viewer UI, crash recovery, or performance limits. Keep R1 release certification closed until those required checks have evidence. |
| Host readiness and novice setup (#378) | The readiness and setup implementation is merged; preflight checks Linux architecture, QEMU/KVM access, root-owned QEMU binaries, and delegated cgroup v2 controllers. | Pop!_OS 24.04 LTS, x86_64, kernel `7.2.15-76070215-generic`, Intel Core i7-1195G7, QEMU/qemu-img 8.2.2, writable `/dev/kvm`; the host preflight and real guest acceptance both passed in a transient delegated user scope. | This proves readiness and the positive boot/action/stream/cleanup path on this host. Novice setup usability and broader isolation/negative tests remain separate evidence. |
| Guest image and authenticated companion (#379, #380, #381) | Generation 19 is the current published signed catalog entry. It includes the merged app-helper cgroup capacity fix and companion protocol 1. | The local real-KVM acceptance verified the signed generation 19 image, authenticated the companion, launched Chromium, captured authenticated guest and display frames, and released the VM successfully. | Candidate build [43557367751](https://github.com/zbarno/kool.ade/actions/runs/43557367751), promotion [43558652176](https://github.com/zbarno/kool.ade/actions/runs/43558652176), catalog PR [#628](https://github.com/zbarno/kool.ade/pull/628), and publication [43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152) completed. Broader guest/host isolation negatives and restart/crash recovery remain unverified. |
| GUI actions, viewer/takeover, guest apps, artifacts, and QA repair (#382, #383, #385, #389, #394) | The component issues are merged with automated contract, policy, artifact, provisioning, and UI coverage. The guest reports accessibility-tree inspection as unsupported in v1. | On 2026-11-14, real KVM accepted mouse/keyboard actions against Chromium, verified changed guest frames, and exercised actor replacement and stale-authority rejection during human control and handback. See [PR #630](https://github.com/zbarno/kool.ade/pull/630) and [#392 comment 6873108849](https://github.com/zbarno/kool.ade/issues/392#issuecomment-6873108849); PR #630 was still awaiting hosted checks. | File bridge, artifact isolation under adversarial input, seeded QA repair, live viewer UI takeover usability, accessibility, and performance measurements still need integrated evidence. Mock and synthetic-browser tests do not prove these paths. |
| R2 public Internet research (#386) | The restricted research network profile and proxy boundary are implemented and covered by policy/component tests. | No real-VM outbound-network, metadata, private-destination, or proxy-bypass matrix is recorded in #392. | Keep claims limited to the implemented profile until real-VM network-denial and allowed-destination evidence is attached. |
| R3 encrypted personal VM and synthetic authentication (#387) | The implementation is merged. The synthetic IdP/MFA/browser-restart job passed in candidate run 43189760047. | No personal persistent VM was accepted on a real host. | `PERSONAL_VM_R3_ENABLED` is `false`; keep personal mode unavailable until its threat-model and real-host evidence are accepted. |
| R4 sensitive external actions (#388) | The policy validator rejects `HighImpactExternalAction` until exact transaction grants exist; denial and stale-approval tests are present. | No checkout, publication, or other external side-effect flow is certified. No real money or real accounts are used in CI. | High-impact external actions remain unsupported. A future gate needs synthetic-account allow/deny, changed-detail, duplicate-execution, and uncertain-outcome evidence. |
| R5 remote VM workers (#390) | Remote worker code merged in [PR #572](https://github.com/zbarno/kool.ade/pull/572). Separate worker identity/stream fixtures merged in [PR #595](https://github.com/zbarno/kool.ade/pull/595); the tests reject wrong host fingerprints and cross-worker sessions. | These fixtures use in-process stdio streams. They are not real SSH connections or separate physical hosts. | `remote_computer_use_release_gate_open_v1()` is `false`; #390 remains open for independent real-host worker, restart, interruption, and competing-operator evidence. |
| R5 macOS and Windows host adapters (#395) | The issue remains open; no adapter is certified. | No macOS or Windows host VM evidence is recorded. | Unsupported. Do not start adapter certification until the verified Linux baseline required by #395 is accepted. |

## Reproducibility record

The current published signed catalog is generation 19,
`computer-use-images-v1-19-53ca225f40a0`, based on
`06baa2475c72d06270a08758326a9af7c17bd856`. The image digest is
`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`,
the companion digest is
`sha256:0f71003e35be1007e2b236a3cb728ef87946bbcd1be10963e13cece15ee272fa`,
and the signed catalog manifest digest is
`sha256:76a1828c8b0fde2978e9b32c83d2ba840a67ae95ab6a5ef8f30ef5666b17d5c4`.
The catalog update merged in [PR #628](https://github.com/zbarno/kool.ade/pull/628)
as `2fac33867ca018d601ee2f93ed219bde9fc106d2` and was published by
[run 43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152).

On 2026-11-14, the local host passed the production real-KVM acceptance test
against that published digest. The host was Pop!_OS 24.04 LTS, Linux x86_64,
kernel `7.2.15-76070215-generic`, 11th Gen Intel Core i7-1195G7, QEMU and
qemu-img `8.2.2`; `systemd-detect-virt --vm` reported `none`, and `/dev/kvm`
was writable. The test ran under
`systemd-run --user --scope --property=Delegate=yes`, downloaded and verified
the 682,426,368-byte image, booted the disposable VM, authenticated companion
protocol 1, launched Chromium, received valid 1280×800 guest and QEMU display
frames, and completed VM release. The command
`KOOLADE_COMPUTER_USE_R1_REAL_KVM=1 cargo +1.98.1 test --locked --lib computer_use_r1_real_kvm_acceptance -- --nocapture`
completed with `1 passed; 0 failed` in 945.19 seconds total; this duration
includes image download, so it is not a provisioning-latency measurement.
The evidence is recorded in [#392 comment 6872727893](https://github.com/zbarno/kool.ade/issues/392#issuecomment-6872727893).

This is positive boot, authenticated launch, observation, display, and release
evidence for one Linux host and one image. It is not evidence for adversarial
guest/host isolation, input actions, file transfer, takeover, crash recovery,
other hosts, or feature tiers that remain disabled. No online Actions runner
labeled `computer-use-kvm` is registered; this acceptance was run locally.

At source commit `dbdf4828bf4a63801189e483bc1b04e644b86c39`, the same opt-in
acceptance test also passed a cached-image guest interaction/handoff run in
47.44 seconds. It used the same host and published image, exercised a click,
Ctrl+L, typed local data URL and Enter, refreshed observations after each input,
and performed guest keyboard input under HumanControl before handing authority
back to the agent. The test verified the screen digest changed and that revoked
agent/human capabilities could not execute after replacement. This checks the
controller/guest action and actor-fencing path; it does not test the live viewer
UI, external manual input, file bridge, or guest/host escape boundaries.

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
- No adversarial guest/host isolation matrix, repeated resource/cleanup run,
  or accepted provisioning/screen/idle performance measurements are recorded
  yet. The successful positive-path run above does not cover these cases.
- At merge time, PRs #593 and #595 were merged by `zbarno`; GitHub had no
  submitted formal review on either PR. The owner may review manually, but no
  independent GitHub review event is asserted here.

Until those gaps are addressed and the corresponding issue evidence is
updated, #392 and #375 stay open and unsupported tiers remain fail-closed.
