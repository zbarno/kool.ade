# Trusted OS package archive keys

`debian-bookworm-archive-keyring.gpg` contains public Debian archive signing
keys used to verify Bookworm `InRelease` metadata. The accepted primary
fingerprints are checked in `src/harness/resource_bridge/debian_repository.rs`:

- `B8B80B5B623EAB6AD8775C45B7C5D7D6350947F8` (Debian 12 archive)
- `04B54C3CDCA79751B16BC6B5225629DF75B188BD` (Debian 13 archive)
- `4D64FEC119C2029067D6E791F8D2585B8783D481` (Debian 12 stable release)

These are public keys published by Debian at
<https://ftp-master.debian.org/keys/>. Update this file and the fingerprint
allowlist together through a reviewed change before repository signing-key
rotation. Unknown signers fail closed.
