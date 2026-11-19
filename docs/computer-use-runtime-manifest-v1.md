# Computer Use runtime manifest v1

Status: implemented behind the app library as
`computer_use::runtime_manifest`, plus an offline ops binary
`computer-use-runtime-manifest`. The macOS packager enforces the
application-pinned trust gate for bundled runtime resources, but no release
ships a signed runtime manifest yet and no Linux adapter consumes one; the
consuming reader today is the bundled macOS QEMU receipt path. Shipping a
signed manifest in a release (provisioned, attested QEMU plus the
repository signing secret), Windows consumption, and a dedicated runtime
signer key remain open work under #395.

The runtime manifest is the signed, app-pinned authorization document that
lets a host adapter trust specific runtime artifacts (such as the bundled
QEMU executable) without falling back to `PATH` lookup, embedded digests, or
operator-supplied hashes. It complements the guest image catalog
(companion to CU-03, `koolade.computer-image-catalog.v1` domain) and shares
its fail-closed posture: a missing, unreadable, oversized, stale, malformed,
or improperly signed manifest yields no verified artifact. Never a weaker
launch path.

## Document shape

Signed JSON (`koolade-computer-runtime-manifest-v1.json` inside the bundle
resource root), maximum 1 MiB, `deny_unknown_fields`, camelCase:

- Header: `schemaVersion` (fixed at 1), `releaseVersion` of the application
  release the manifest was built for, monotonic `generation`,
  `issuedAtUnixMs`/`expiresAtUnixMs` with a hard 180-day maximum lifetime,
  and the UUID `signerKeyId`.
- `artifacts`: 1..=32 entries. Each entry carries a unique UUID
  `artifactId`, a `kind` (`executable`, `file`, or `disk_image`), a
  `platform` (`linux_x86_64`, `linux_aarch64`, `mac_os_intel`,
  `mac_os_apple_silicon`, `windows_x64`, `windows_arm64`), the
  `guestArchitecture` the platform requires (cross-checked), a
  `bundleRelativePath` restricted to canonical POSIX relative paths (at most
  256 bytes, at most 8 segments, no `.`/`..`, restricted character set), a
  `sha256:` prefixed lowercase-hex digest of the exact shipped bytes, a
  `sizeBytes` capped at the 16 GiB catalog ceiling, and `permissionBits`
  that must be zero for non-executable kinds. Executable modes must not set
  setuid/setgid/sticky, must grant owner execute, and must not grant
  group/other write.
- `signatureHex`: 128 lowercase hex characters.

Signing domain is `koolade.computer-runtime-manifest.v1\0` prepended to the
canonical (sorted-key) unsigned JSON. A signature minted under the guest
image catalog domain or any other domain is structurally unverifiable here,
and vice versa. Envelope consumers re-hash the complete envelope (signature
included) into the manifest digest that receipts record as provenance.

Verification (`verify_runtime_manifest`) is the only admission path and
enforces: size bound, strict decode, header/artifact rules, uniqueness of
`(artifactId)` and `(platform, path)`, refusal of future-dated issuance, expiry against caller-provided host
time, the pinned-trust minimum-generation floor, known-signer lookup, and
Ed25519 verification against app-pinned keys
(`RuntimeManifestTrustV1::from_application_pinned_keys`). Nothing in the
library discovers artifacts on disk; readers bind the on-disk state
themselves at use time (the macOS receipt re-checks presence, symlink
refusal, permissions, size, and digest parity through
`MacOsTrustedQemuRuntimeV1::path_for_plan`, and
`MacOsTrustedQemuRuntimeV1::from_manifest_artifact` converts a verified
manifest entry into a receipt carrying `manifest_provenance` digest plus
artifact ID).

Canonical QEMU executable paths are fixed per platform
(`qemu/qemu-system-x86_64`, `qemu/qemu-system-aarch64`, and their
`*.exe` Windows forms); lookup is exact equality. Platforms whose adapters
have not adopted the manifest (currently the Linux adapters) map to an
empty canonical path and therefore have no artifacts resolvable through it.

## Offline ops tool

`cargo run --bin computer-use-runtime-manifest` provides four commands:

- `sign RELEASE_VERSION GENERATION ISSUED_MS EXPIRES_MS KEY_ID
  TEMPLATE_JSON OUTPUT_JSON` — reads a 32-byte Ed25519 seed as hex from
  stdin (optionally prefixed `ed25519-seed-hex:`), fills the template,
  signs the domain-tagged payload, and writes the envelope. The signer must
  hold a key the app pins; the seed is zeroized after use.
- `verify BUNDLE_ROOT_DIR PIN_JSON NOW_MS` — loads the fixed-name bundled
  manifest and verifies it under the trust pin. Emits the bound digest,
  release, generation, signer, expiry, and artifact summary on success.
- `verify-pinned BUNDLE_ROOT_DIR` — verifies the fixed-name bundled manifest
  against THIS build's application-pinned trust (no operator pin involved).
  Absence of the manifest is a refusal, never a pass. This is the
  machine-checkable half of the shipped bundle resource gate below.
- `inspect FILE` — pretty-printed passthrough for operators.

The trust pin is explicit operator-selected JSON:

```json
{
  "minimumGeneration": 1,
  "keys": [
    {
      "keyId": "<uuid>",
      "publicKeyHex": "<64 lowercase hex>"
    }
  ]
}
```

Until a dedicated runtime signer key is provisioned in a reviewed app
release, the release signing workflow reuses the image-catalog key material
under the distinct runtime signer key ID
`9b1c6e04-7a2f-4f3d-bc5b-2d4e6f8a0c1e`; domain tagging provides the
cryptographic separation. Key rotation raises the generation floor; old
generations fail closed after the floor moves past them.

## Shipped bundle resource gate

Because the repository never holds the pinned key seed, a bundle resource
that satisfies `verify-pinned` is the only kind of QEMU staging a packager
may embed. `scripts/package-macos-dmg.sh` exposes exactly this seam: with
`KOOADE_BUNDLED_RUNTIME_DIR` set, it runs `verify-pinned` over the staging
directory (layout: the fixed-name manifest at the top level beside the
`qemu/` subtree it describes), stages the admitted subtree into the app
`Resources`, and appends the verification transcript to the package
inspection log; any failure aborts packaging before an artifact is ever
cut. With the variable unset, packaging behaves exactly as before and the
resulting app carries no bundled runtime resources — which is a truthful
statement about what that release certifies.

Consequences kept deliberately sharp: CI cannot mint an admissible manifest
(the secret lives only with the promotion pipeline), a repackaged manifest
breaks the signature and the digest both, and the app-side reader still
fails closed independently of whatever a packager believed.
