# Computer Use readiness probe boundary (v1)

The machine setup scan now emits stable `setup:computer:*` task findings as a
separate readiness track. The positive host checks require observed x86_64
Linux architecture, no detected nested hypervisor, an actual read/write open
of `/dev/kvm`, and successful `/usr/bin/qemu-system-x86_64 --version` and
`-accel help` responses, including the reported KVM accelerator capability.
The overlay prerequisite also verifies root-owned `/usr/bin/qemu-img` and its
bounded `--version` output with a two-second timeout; missing, untrusted,
malformed, or timed-out results keep the QEMU task blocked or unverified.
The launcher check reuses the backend's read-only validation for the fixed
`/usr/bin/qemu-system-x86_64` path, root-owned executable, non-writable mode,
no setuid/setgid bits or file capabilities, and trusted `/`, `/usr`, and
`/usr/bin` ancestors. The backend probe also confirms its child seccomp filter
can be installed. Session launch repeats executable validation immediately
before spawn.
Nested hypervisors are checked using the fixed read-only
`/usr/bin/systemd-detect-virt --vm` probe; a detected VM is Unsupported, and
only the matching `none` output with its no-VM exit status permits the KVM
device check. Missing, malformed, or inconsistent output and status stay
Unknown. The command and each QEMU probe have a two-second timeout.
QEMU probes drain stdout while retaining at most 4 KiB and discard stderr.
CPU virtualization flags by
themselves are never treated as KVM readiness. The scan does not start a VM or
change host state. Raw command output is not persisted; only a bounded first
version line with the standard `QEMU emulator version major.minor.patch` form
and the exact `kvm` accelerator entry are interpreted. Malformed version text
is Unknown; an old but well-formed version remains Unverified until the VM
backend defines a tested minimum. These
bounded values and all unavailability outcomes are serialized as typed
generation-scoped evidence without retaining executable paths. A minimum
QEMU version has not been established by the VM backend, so the version is not
presented as a compatibility guarantee.

CPU virtualization flags are read from a bounded `/proc/cpuinfo` input and
serialized with the nested-hypervisor probe as typed supporting evidence; they
do not prove KVM availability. KVM open
results, including missing device, permission denial, nested virtualization,
and unknown outcomes, are serialized as typed Linux host evidence bound to the
current scan generation. Snapshot validation accepts `computer:kvm` only when
the `Host/Linux/LinuxKvmDevice/None` evidence is `Ready`, uses
`LinuxKvmDeviceAccessible` from `HostRuntime`, and contains
`LinuxKvmDevice { accessible: true }`. Typed QEMU evidence retains the bounded
version and KVM accelerator result while keeping a missing, denied, or unknown
probe distinct. It also stores the independent `qemu-img` and fixed-launcher
trust outcomes; launcher failure blocks QEMU readiness even when the system
emulator reports a valid version. The Welcome readiness screen displays an ability matrix
for Docker coding, disposable computer, internet computer, personal encrypted
computer, and interactive viewer. It reports an ability as Available only when
the current scan supplies positive Docker/KVM evidence and each remaining
named prerequisite task has a persisted Verified state. Absent or incomplete
evidence remains Blocked, and an Unsupported prerequisite remains Unsupported.
This presentation is informational and does not authorize execution or repair.
The QEMU setup action is available only for the supported Debian 13, Ubuntu
24.04, and Pop!_OS 24.04 x86_64 recipes. Before consent, it validates active
APT sources, signed official archive candidates, and the simulated install
dependency closure; every package selected by APT must resolve to an approved
distribution archive. After explicit administrator consent, it refreshes
archive metadata and repeats those checks before installing only the fixed
`qemu-system-x86` and `qemu-utils` package pair, without upgrades, removals,
or recommended packages. The broker accepts no shell command or caller-selected
package. Other computer host-changing operations remain unavailable until they
have fixed native implementations, exact task binding, fresh preconditions,
and post-change verification. Firmware, group membership, and blanket
permission changes are never automated by this readiness flow.

KVM missing, permission denied, unsupported nested virtualization, and
uncheckable states remain distinct. QEMU is probed independently, and the
reviewed package action is offered on the three supported Linux recipes when
QEMU is missing or lacks KVM acceleration. The disposable VM ability remains
blocked until both QEMU and KVM are verified.
The guest-image library exposes a pre-session signed catalog verifier and a
read-only cached-artifact integrity check. The scan reads the bundled image
lock without host access: this build's lock is explicitly `not_promoted` and
empty, so the image task is unsupported. A malformed or promoted-but-unverified
lock remains unknown. The image task gates storage and viewer tasks; storage
also depends on the QEMU task because `qemu-img` is required for overlays.
Storage gates personal encryption. Docker execution
readiness is assessed independently, so a VM finding does not make ordinary
Docker work unavailable. Its state comes from the completed, persisted setup
scan; UI rendering does not launch a Docker probe.

Internet computer mode has a separate `setup:computer:network` task. A local
filtered HTTPS gateway is implemented for explicit disposable InternetResearch
grants, but the current signed guest companion protocol is v1, so consent does
not offer networking and the disposable computer profile stays offline. The
network readiness task remains Unverified until a compatible signed image is
promoted and the independent real-guest security matrix in #392 passes.

Current typed Linux kernel and user namespace observations are consumed by the
KVM finding; a missing or inaccessible current-generation observation remains
Unknown. A bounded read of `/proc/meminfo`, capped by the current cgroup memory
availability, and read-only `statvfs` against the planned private Computer Use
data directory's nearest existing parent records available memory and storage.
The same parent is inspected with `statfs`; a bounded filesystem-family value
(ext, XFS, Btrfs, or other) is recorded when available. A `statfs` failure does
not erase a successful free-space measurement. Normal kernel path resolution is
used for configured storage roots; no path is persisted, and the probe creates
no directories. The family value does not prove qcow2 behavior or overlay
capacity. The initial host floor is 1 GiB RAM and
2 GiB disk, matching the v1 minimum disposable guest budget. A known shortfall
blocks the storage card; adequate host capacity and a known filesystem remain
Unverified until the promoted signed image size and private overlay capacity
are verified. The VM launcher must recheck the exact task policy budget
immediately before startup. Unverified tasks cannot be marked ready by a
provider response. macOS
and Windows are classified unsupported for this v1 support claim. Nested
virtualization detection remains dependent on the host's systemd detection
utility. Real VM startup and host isolation remain in #392's hardware release
matrix; those owner-run checks are not a closure prerequisite for this issue.

The machine snapshot also records a typed current-build readiness-probe
inventory for guest image catalog integrity, filesystem overlay support,
filtered network gateway, authenticated graphics transport, personal
encryption key service, and guest protocol compatibility. Each record is bound
to the current scan generation and has no host path or raw command output. The
readiness probes for a filtered gateway, authenticated viewer, and personal key
service are `Unsupported` because those components are not integrated in the
readiness scan in this build. The filtered gateway implementation is not
itself evidence of signed-image compatibility or real-guest isolation. Image
catalog integrity is `Missing` when the bundled lock confirms no
image was promoted. A promoted catalog is `Ready` only when its app-pinned
signature, lock digest and generation, existing durable generation floor, and
cached image bytes all verify. Missing or unsafe floor/cache state and malformed
or unverified promoted locks remain `Unknown`. Readiness reads the existing
cache and floor without creating or repairing them.
Actual filesystem overlay capacity and guest protocol compatibility are
`Unknown` until the signed image and VM components provide verifiable evidence.
Snapshot validation rejects positive claims and requires the complete inventory
for Linux VM scans, with every record bound to the enclosing scan generation.
The overlay creation API exists, but the readiness scan does not verify host
filesystem overlay support or capacity.

The deterministic fixture tests cover stable task IDs/dependencies, CPU flags
not substituting for an openable KVM device, missing/denied/indeterminate KVM
opens through the fact-to-card path, stale kernel/user-namespace evidence after
a rescan, absent/malformed/old QEMU outcomes, a disabled user namespace
blocker, and absent image/storage/viewer/vault evidence never becoming ready.
Snapshot validation requires typed KVM, QEMU, storage, and current-build
component observations; task
reconciliation round-trips persisted computer task IDs and dependencies
through serialization and rescans without duplicate cards. Resource evidence
binds any filesystem-family observation to a successful free-storage reading;
the read-only probe tests nearest-existing-parent lookup and normal symlink
resolution without creating the planned path.
The Linux timeout fixture kills a descendant holding the probe output pipe.
These checks do not prove actual QEMU/KVM startup or security isolation.
