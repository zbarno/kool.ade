# Computer Use host adapter design

Status: implementation design. macOS and Windows host virtualization remain
unavailable. This document records candidate hypervisors and the security
conditions an adapter must meet before it can be enabled.

## Candidate host and guest pairs

| Host | Initial guest architecture | Candidate accelerator | Required host prerequisite |
| --- | --- | --- | --- |
| macOS Intel | x86_64 Linux | QEMU HVF | Native Intel host identity and successful HVF capability probe |
| macOS Apple silicon | aarch64 Linux | QEMU HVF | Native Apple silicon identity and successful HVF capability probe |
| Windows x64 | x86_64 Linux | QEMU WHPX | Windows Hypervisor Platform installed and available |

QEMU documents HVF for macOS on x86 and Arm hosts. Its WHPX backend uses the
Windows Hypervisor Platform and requires that Windows optional feature. QEMU
documents Windows 10 version 2004 as its tested minimum for x86_64 WHPX. A
backend must check the native host architecture, not the process architecture;
an x86_64 macOS process running under Rosetta is an Apple silicon host.

The initial guest architecture must match the host architecture. An Apple
silicon backend must not silently boot the existing x86_64 Linux image through
software emulation. Cross-architecture translation inside an arm64 guest is a
separate, explicitly reviewed capability.

QEMU/HVF and QEMU/WHPX are candidates, not approved isolation designs. Apple’s
Virtualization framework is another macOS option, but its use requires the
`com.apple.security.virtualization` entitlement and architecture-compatible
guest images. Selecting that framework therefore also requires signed app
entitlement and distribution work.

## Shared authority and host boundary

Each adapter must implement the existing `ComputerBackend` contract and retain
the signed session, lease generation, image catalog, guest handshake, action
journal, evidence ownership, and user takeover checks. No platform adapter may
accept model-provided paths, executable arguments, socket addresses, or
hypervisor controls.

Each session must have an app-owned private directory, a bounded disk overlay,
bounded memory and CPU, and a process or VM containment object whose lifetime
is tied to the session. Cancellation and recovery may act only on a VM whose
identity, image, session, lease, and owner all match the durable record.
Personal persistent sessions remain disabled until a platform key-storage and
recovery design is separately accepted.

The initial VM has no host, bridge, or user-mode network device. Future network
profiles must use the app-owned policy gateway and pass the cross-platform
private-address, redirect, DNS, IPv6, and metadata denial suite. A platform
adapter must not replace these checks with a guest firewall or a QEMU command
line assertion alone.

Guest control and display streams must be private to the current user, use
bounded I/O timeouts, authenticate the guest boot transcript, and reject
remote clients. Windows named pipes require an explicit per-user ACL and peer
verification. Do not expose QMP, VNC, or the guest control channel on a TCP
listener. File exchange continues through the existing typed, quota-bounded
grant protocol; the VM must not receive an unrestricted host directory mount.

## Platform-specific prerequisites

### macOS

- Probe Intel versus Apple silicon host identity and HVF availability
  independently. Keep the backend unavailable if the result is unknown.
- Use an architecture-matched signed guest image and the existing signed
  companion protocol. Do not treat Rosetta process translation as VM
  acceleration evidence.
- Provide an enforceable per-session CPU, memory, disk, and process boundary.
  POSIX file permissions and QEMU `-m`/`-smp` arguments alone do not establish
  the complete host containment boundary.
- Keep host display, input, private storage, and keychain access out of the
  guest unless a separately reviewed typed capability authorizes them.
- If using Apple’s Virtualization framework, package and verify the required
  entitlement before any readiness claim.

### Windows

- Support Windows x64 first. Windows ARM64 is a separate architecture gate.
- Probe WHPX installation and runtime availability; never enable Windows
  optional features or alter enterprise virtualization settings without
  explicit typed consent.
- Put each QEMU process in an app-owned Job Object with verified CPU, memory,
  process, and kill-on-close limits. Fail closed if assignment or limits cannot
  be confirmed.
- Create private per-user named pipes for guest control and display. Reject
  remote pipe clients and validate the connecting process identity.
- Canonicalize all runtime and overlay paths and reject reparse points,
  reserved device names, and paths outside the app-owned data root.

## Required deterministic tests

Platform fixtures must cover native architecture selection (including Rosetta),
missing accelerator, missing Windows feature, unsupported guest architecture,
invalid image signatures, weak or absent resource limits, cross-user pipe
access, remote pipe clients, path traversal/reparse points, accidental network
devices/listeners, invalid guest transcripts, transfer quota violations,
process/VM loss, cancellation, and stale lease recovery. A synthetic fixture
can verify contracts but cannot certify a host.

Real macOS Intel, macOS Apple silicon, and Windows x64 evidence belongs in the
cross-tier matrix in [#392](https://github.com/zbarno/kool.ade/issues/392).
Until each host’s adapter, deterministic tests, and independent host evidence
pass, setup and session APIs must report the host as unavailable and must not
fall back to executing an agent on the host.

## References

- [QEMU supported acceleration frameworks](https://www.qemu.org/docs/master/system/introduction.html)
- [QEMU Windows Hypervisor Platform backend](https://www.qemu.org/docs/master/system/whpx.html)
- [Apple Virtualization framework](https://developer.apple.com/documentation/virtualization)
- [Apple Virtualization entitlement](https://developer.apple.com/documentation/virtualization/adding-the-virtualization-entitlement-to-your-project)
