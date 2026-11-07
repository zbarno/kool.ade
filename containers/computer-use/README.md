# Computer Use VM image candidates

`build-image.sh OUTPUT_DIR` builds an app-owned, unsigned Debian Bookworm GUI
candidate from the fixed snapshot in the script. CI runs it twice and requires
identical image bytes, SBOM, package list, QEMU metadata and recipe records
before keeping a candidate. The final ext4 filesystem is assembled offline
after GRUB setup; inode and superblock times are normalized, the MBR disk ID is
fixed, the generated ldconfig auxiliary cache and build-time guest logs are
removed, and the stored mount path is replaced with a fixed marker. Builder
utility versions are recorded, but cross-toolchain reproducibility is not
claimed. The recipe never consumes task source. It requires Linux, sudo,
`debootstrap`, QEMU tools, GRUB, filesystem utilities, and network access to
the pinned Debian snapshot.

The builder verifies that both root and the desktop user's password are
locked, that the desktop account is non-root, and that it has no `sudo`
membership before packaging the disk.

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

The output is not deployable. The application pins a catalog verification key,
but this candidate has no signed manifest or promoted lock entry. The companion
executable and real-KVM evidence also remain outstanding through #381 and #392;
#378 consumes the promoted catalog for readiness. `images.lock.json`
deliberately stays empty with `not_promoted`; this VM catalog and workflow are
separate from Docker's image lock and release path. A manual build on `main`
receives GitHub OIDC provenance and package-inventory attestations, but those do
not replace the app-trusted catalog signature. Promote only through a later
reviewed change that provides a signed catalog bound to the exact companion
payload and all required readiness evidence.
