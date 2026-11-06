# Computer Use VM image candidates

`build-image.sh OUTPUT_DIR` builds an app-owned, unsigned Debian Bookworm GUI
candidate from the fixed snapshot in the script. It requires Linux, sudo,
`debootstrap`, QEMU tools, GRUB, partition/ext4 utilities, and network access to
the Debian snapshot; it does not consume task source. The workflow installs
these tools and uploads a short-lived review artifact.

The output is not deployable. The companion executable, app-pinned catalog key,
signed image manifest, promotion lock digest, and real-KVM evidence are missing
until #381 and #392 land. `images.lock.json` deliberately stays empty with
`not_promoted`; this VM catalog and workflow are separate from Docker's image
lock and release path. A manual build on `main` receives GitHub OIDC provenance
and package-inventory attestations, but those do not replace the app-trusted
catalog signature. Promote only through a later reviewed change that provides
the app-trusted signed catalog and all readiness evidence.
