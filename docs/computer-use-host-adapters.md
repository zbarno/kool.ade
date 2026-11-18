# Computer Use host adapter design

Status: implementation design with offline macOS QEMU/HVF planning and a
fail-closed Windows AF_UNIX guest-control transport candidate.
macOS and Windows host virtualization remain unavailable. The launch planner
does not start QEMU or establish process, resource, or storage containment.
This document records candidate hypervisors and the security conditions an
adapter must meet before it can be enabled.

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
remote clients. Windows local sockets require an explicit per-user ACL and
verification that the peer is the active QEMU process. Do not expose QMP, VNC,
or the guest control channel on a TCP listener. File exchange continues through
the existing typed, quota-bounded grant protocol; the VM must not receive an
unrestricted host directory mount.

## Platform-specific prerequisites

### macOS

- The current macOS code produces a fixed, offline QEMU launch plan for the
  native host architecture: `qemu-system-x86_64` with `q35` on Intel, and
  `qemu-system-aarch64` with `virt` on Apple silicon. Both plans require HVF,
  disable guest networking and QEMU defaults, validate CPU/memory/disk inputs
  and overlay size against the session budget, and accept only absolute,
  restricted-character path strings without QEMU option separators. Unix
  socket paths are limited to the macOS `sun_path` bound.
- This planner is not connected to a process launcher or `ComputerBackend`.
  It does not verify path ownership/symlinks, image architecture/provenance,
  QEMU binary trust, VM resource enforcement, guest/display socket peer
  identity, cancellation, or recovery. No plan may be executed until those
  controls are implemented and reviewed; `release_enabled` remains false.
- The bounded process primitive accepts a fixed macOS plan only with an
  absolute architecture-matched executable path and an open host release gate.
  It passes the plan's argument vector directly to the child, without shell
  parsing or ambient `PATH` lookup. Guest RAM comes from the plan, capped at
  16 GiB; the process inherits the session disk budget as `RLIMIT_FSIZE` to
  cap individual child-created files and runs with a private process group,
  CPU-time limit, no core dump, restricted file descriptors, and a private
  creation umask. `RLIMIT_FSIZE` is not an aggregate filesystem quota. Darwin
  does not support `RLIMIT_AS`, so this does not enforce a host process memory
  cap. The caller still has to resolve and verify an app-trusted QEMU runtime.
  Binary provenance, authenticated display/control transport, complete recovery, and
  a usable `ComputerBackend` lifecycle remain incomplete.
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
- Keep QEMU's built-in Windows pipe unavailable unless its per-user ACL, remote
  client rejection, and peer identity are verified. The current transport
  candidate uses an app-owned AF_UNIX listener and checks the connected peer PID
  against the active QEMU child.
- Bind the exact session socket below a private, short per-user path. Validate
  its AF_UNIX reparse tag, owner, ACL, and path before accepting a connection.
- Keep VM startup disabled until a pinned, trusted Windows QEMU build passes
  native client-mode and peer-PID integration checks.
- Canonicalize all runtime and overlay paths and reject reparse points,
  reserved device names, and paths outside the app-owned data root.

The current #395 Windows slices add a deterministic QEMU/WHPX launch plan,
resource mapping, protected storage, and an app-owned AF_UNIX listener. The
session-bound constructor verifies the controller signature, exact Windows x64
host and WHPX target, disabled-network policy, and overlay session/image
provenance before producing the plan. The plan fixes the guest to WHPX, an
x86_64 machine, a qcow2 overlay, one virtio-serial
control endpoint, and no network or display listener. It caps guest vCPU and RAM
from the signed session budget and configures Job Object CPU, process-memory, and
process-count limits. The socket is placed under a short private per-user path
because Windows AF_UNIX addresses have a fixed path-length limit.

After accept, the listener queries the Windows AF_UNIX peer PID and requires it
to match the live QEMU child before constructing the guest channel. The socket
leaf is separately checked for its exact session location, reparse tag, owner,
and private ACL. Native Windows fixtures cover those checks and the peer-PID
query. The native CI job also installs the QEMU package through MSYS2's signed
package manager and runs QEMU's actual AF_UNIX client chardev inside the
production Job Object/process-to-listener fixture. This verifies the QEMU
client mode and connected peer identity on Windows; it does not verify WHPX VM
boot, QEMU packaging for application releases, guest display/input, or the
production guest image. The runtime gate remains closed until a versioned,
application-trusted QEMU binary is pinned and consumed by a complete lifecycle
adapter. These slices do not start the production VM, implement a display/input
proxy, provide a Windows disk quota, or complete native host acceptance.
Guest-visible virtual disk capacity is bounded by the session policy, but
physical qcow2 growth is not a host disk quota.

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
- [QEMU Windows named-pipe behavior](https://www.qemu.org/docs/master/system/qemu-manpage.html)
- [Microsoft named-pipe security and access rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
- [Apple Virtualization framework](https://developer.apple.com/documentation/virtualization)
- [Apple Virtualization entitlement](https://developer.apple.com/documentation/virtualization/adding-the-virtualization-entitlement-to-your-project)
