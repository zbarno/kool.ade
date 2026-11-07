# Computer Use VM image candidates

`build-image.sh OUTPUT_DIR AGENT_BINARY` builds an app-owned, unsigned Debian
Bookworm GUI candidate with the authenticated guest companion from the fixed
snapshot in the script. CI builds the Rust companion with toolchain 1.98.1,
runs the image recipe twice, and requires
identical image bytes, SBOM, package list, QEMU metadata and recipe records
before keeping a candidate. The final ext4 filesystem is assembled offline
after GRUB setup; inode and superblock times are normalized, the MBR disk ID is
fixed, the generated ldconfig auxiliary cache and build-time guest logs are
removed, and the stored mount path is replaced with a fixed marker. Builder
utility versions are recorded, but cross-toolchain reproducibility is not
claimed. The recipe never consumes task source. It requires Linux, sudo,
`debootstrap`, QEMU tools, GRUB, filesystem utilities, and network access to
the pinned Debian snapshot. The guest imports a private per-boot fw_cfg file
and starts the companion as the unprivileged `koolade` account.

The two-build workflow check covers the production recipe's repeatability
boundary: separate image builds from one revision and snapshot must produce
identical qcow2 bytes and supporting metadata. Unit tests cannot prove this
because package installation, filesystem creation, GRUB and qcow2 conversion
all contribute to the artifact. It uses an ephemeral Ubuntu runner, loop
devices, sudo, and network access to the pinned Debian snapshot; setup cost is
high because it builds the guest twice. This same-run check does not establish
reproducibility across different builder toolchain versions.

The output is not deployable. The app-pinned catalog key, signed image
manifest, promotion lock digest, and real-KVM evidence remain separate
acceptance work. `images.lock.json` deliberately stays
empty with `not_promoted`; this VM catalog and workflow are separate from
Docker's image lock and release path. A manual build on `main` receives GitHub
OIDC provenance and package-inventory attestations, but those do not replace the
app-trusted catalog signature. Promote only through a later reviewed change
that provides the app-trusted signed catalog and all readiness evidence.
