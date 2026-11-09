# Computer Use VM image candidates

`build-image.sh OUTPUT_DIR AGENT_BINARY APP_HELPER_BINARY [APP_PACKAGE_DIR]` builds an app-owned, unsigned Debian
Bookworm GUI candidate with the authenticated guest companion from the fixed
snapshot in the script. CI builds the Rust companion with toolchain 1.98.1,
runs the image recipe twice, and requires
identical image bytes, SBOM, package list, QEMU metadata and recipe records
before keeping a candidate. The companion and app-helper binary digests are
recorded in sidecars and included in the recipe digest. The final ext4 filesystem is
assembled offline after GRUB setup; inode and superblock times are normalized,
the MBR disk ID is fixed, the generated ldconfig auxiliary cache and build-time
guest logs are removed, and the stored mount path is replaced with a fixed
marker. The desktop user and authenticated companion have separate UIDs. The
boot secret and virtio port belong only to the companion identity; a root-owned
local helper accepts requests only from that UID and launches fixed GUI app IDs
as the unprivileged desktop user. An optional app package directory accepts
only regular `<sha256>.deb` files, checks their content digests and Debian
package metadata, and enforces 64-package, 512 MiB per-package and 1 GiB total
limits. The offline packages are copied as non-executable root-owned cache
blobs; the signed catalog is injected separately by the host for each matching
image. Builder
utility versions are recorded, but cross-toolchain reproducibility is not
claimed. The recipe never consumes task source. It requires Linux, sudo,
`debootstrap`, QEMU tools, GRUB, filesystem utilities, and network access to
the pinned Debian snapshot. The guest imports a private per-boot fw_cfg file
and starts the companion as the unprivileged `koolade` account.

Before packaging, the builder verifies that root and desktop passwords are
locked, desktop and companion UIDs differ, neither account is root, and neither
account has `sudo` membership.

The two-build workflow check covers the production recipe's repeatability
boundary: separate image builds from one revision and snapshot must produce
identical qcow2 bytes and supporting metadata. Unit tests cannot prove this
because package installation, filesystem creation, GRUB and qcow2 conversion
all contribute to the artifact. It uses an ephemeral Ubuntu runner, loop
devices, sudo, and network access to the pinned Debian snapshot; setup cost is
high because it builds the guest twice. This same-run check does not establish
reproducibility across different builder toolchain versions.

The workflow also runs `test-overlay-isolation.sh` against real `qemu-img` and
`qemu-io`. It creates a small qcow2 base and two overlays, writes through one
overlay, then verifies the sibling overlay and base still read as zero and the
base hash is unchanged. This tests QEMU's actual copy-on-write behavior, which
the API's fake-process unit tests cannot establish. It needs `qemu-utils` but
no KVM, Docker, network, or SSH; setup cost is low.

The output includes the authenticated companion payload, but is not deployable.
The app-pinned catalog key, signed image manifest and promotion lock digest are
still missing, as is real-KVM evidence for GUI and companion readiness under
#392. `images.lock.json` deliberately stays empty with `not_promoted`; this VM
catalog and workflow are separate from Docker's image lock and release path. A
manual build on `main` receives GitHub OIDC provenance and package-inventory
attestations, but those do not replace the app-trusted catalog signature.
Promote only through a reviewed change that binds a signed catalog to the exact
companion payload and all required readiness evidence.
