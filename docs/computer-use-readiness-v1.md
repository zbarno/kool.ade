# Computer Use readiness probe boundary (v1)

The machine setup scan now emits stable `setup:computer:*` task findings as a
separate readiness track. The positive host checks require observed x86_64
Linux architecture, no detected nested hypervisor, an actual read/write open
of `/dev/kvm`, and successful `/usr/bin/qemu-system-x86_64 --version` and
`-accel help` responses, including the reported KVM accelerator capability.
Each QEMU command has a two-second timeout, drains stdout while retaining at
most 4 KiB, and discards stderr. CPU virtualization flags by
themselves are never treated as KVM readiness. The scan does not start a VM or
change host state. Raw command output is not persisted; only a bounded first
version line and the exact `kvm` accelerator entry are interpreted. A minimum
QEMU version has not been established by the VM backend, so the version is not
presented as a compatibility guarantee.

These checks currently flow into task findings and a checked domain for an
opened KVM device. They do not yet have a serialized Computer Use observation
type in the machine snapshot. This slice also does not display the requested
per-mode ability matrix or route remediation through the typed native
remediation broker.

KVM missing, permission denied, unsupported nested virtualization, and
uncheckable states remain distinct. QEMU availability depends on the KVM task.
The image task remains unverified until its signed catalog is connected; it
gates storage and viewer tasks, and storage gates personal encryption. Docker execution
readiness is assessed independently, so a VM finding does not make ordinary
Docker work unavailable.

Kernel flags, user namespace readiness, image integrity/catalog, hypervisor
containment, gateway, graphics transport, filesystem overlay capacity, RAM and
disk reservations, encryption/key service, and guest protocol probes remain
unimplemented or unverified until their owning image and VM backend components
exist. Unverified tasks cannot be marked ready by a provider response. macOS
and Windows are classified unsupported for this v1 support claim. Nested
virtualization detection is a conservative `/sys/hypervisor/type` heuristic;
real VM startup and host isolation remain part of #392's hardware release
matrix.

The deterministic fixture tests cover stable task IDs/dependencies, CPU/host
readiness not substituting for an openable KVM device, and unsupported platform
classification. They exercise the finding builder; they do not prove actual
QEMU/KVM startup or security isolation.
