# Computer Use guest image catalog and cache (CU-03)

Status: host-side verifier/cache/overlay APIs, authenticated guest companion,
candidate image build, and protected signed-catalog promotion workflow are
implemented. `main` contains signed catalog generation 1 with an approved
x86_64 image and a promoted image lock. The merge-triggered publisher
succeeded in [run #42612937221](https://github.com/zbarno/kool.ade/actions/runs/42612937221)
after [PR #555](https://github.com/zbarno/kool.ade/pull/555) merged. The
[public release](https://github.com/zbarno/kool.ade/releases/tag/computer-use-images-v1-1-6ae802274b9b)
contains the qcow2 image, signed catalog, image lock, SBOM, and verified
attestations. Its approved x86_64 image digest is
`sha256:6ae802274b9bea94f4365fc9a4923cff0496039fd4a283bb95e87fe8bfce127c`.
Real KVM GUI readiness and overlay validation are tracked separately in #392.
The current host support claim is x86_64 Linux without nested virtualization.
Readiness reports macOS, Windows, ARM hosts, and nested virtualization as
unsupported. Image availability alone does not imply host support; future
macOS/Windows backends belong to #395, and platform claims follow the #392
release matrix.

## Image build boundary

`containers/computer-use/build-image.sh` is an app-owned build recipe. It uses a
fixed Debian Bookworm snapshot and installs a graphical Openbox/LightDM/Chromium
guest, UTC locale/time settings, a locked non-root `koolade` account, and the
authenticated companion binary. QEMU passes a fresh session-bound boot config
through a private fw_cfg entry; a root one-shot imports it into a protected
runtime directory, then the unprivileged companion authenticates over the
dedicated virtio-serial port. It accepts only an output directory and the
release-built companion binary; it does not inspect task repositories,
Dockerfiles, or model input. The builder emits a qcow2 candidate, deterministic CycloneDX package-component inventory, qemu-img metadata, recipe hash, and candidate-only status. The inventory does not claim OS-file coverage or license compliance. The
snapshot is deliberately a build input: update it on the security maintenance
cadence, inspect the resulting package inventory, and rerun the isolated image
validation before any promotion.

`.github/workflows/computer-use-images.yml` runs only on an explicit dispatch or
when the recipe changes in a pull request. It retains a candidate artifact and
deterministic CycloneDX package-component inventory for review. A manual run
from `main` adds GitHub OIDC build-provenance and SBOM attestations. Those
attestations do not substitute for the app-pinned Ed25519 catalog signer. The
candidate is not a runtime catalog and does not change the Docker lock, Docker
promotion workflow, or Docker images. The separate
`.github/workflows/computer-use-image-promotion.yml` workflow accepts only a
successful candidate run from the current `main` revision. It verifies image
provenance and the CycloneDX SBOM attestation, rebuilds the companion from that
revision, and checks its digest against the candidate sidecar. It signs with
`COMPUTER_USE_CATALOG_SIGNING_KEY`, then verifies the signature with the
application-pinned public key before creating a draft release and a review
branch that changes only the catalog and guest image lock. Open that branch as
a PR through GitHub so normal required checks run; the promotion job does not
create a PR with `GITHUB_TOKEN`, which would suppress those checks. A wrong or
missing secret fails before the branch is pushed. The secret is never written
to the repository or release assets. The merge-triggered job verifies the merged
catalog and lock against the draft release contents and guest image digest
before publishing that release. A collision with an existing release tag
fails closed. The component inventory does not claim OS-file coverage or
license compliance.
`containers/computer-use/images.lock.json` is separate from
`containers/images.lock.json` and pins the image in signed catalog generation
1. The catalog and lock are present in `main`; the publisher verified them
against the release assets and verified the guest image bytes before making
the release public. A push to `main` that changes the catalog, lock, or
promotion workflow starts that publisher and fails closed on any mismatch. The
current published release is linked above; a newly promoted digest must still
pass the same merge-triggered verification before it is reported as public.

The bootstrap and companion units are installed. The companion runs as the
unprivileged `koolade` account and reconnects after channel loss. The signed
catalog binds the reviewed companion protocol version and binary SHA-256; the
promotion workflow publishes the matching release only after verifying its
assets. The typed channel carries
authenticated heartbeats and structured guest failure reasons; a failed action
leaves its durable host journal entry pending so callers cannot infer that a
side effect did not occur.

## Signed catalog interface

The host API is `computer_use::guest_image`. `GuestImageManifestV1` is UTF-8
JSON with a strict v1 schema, catalog version and monotonic generation, issue
time, UUID signer key ID, architecture-specific image entries, and a detached
Ed25519 signature. The signature covers canonical JSON (sorted JSON object
keys) prefixed by `koolade.computer-image-catalog.v1\0`; the manifest digest is
SHA-256 over canonical JSON including the signature. `GuestImageManifestTrustV1`
can only be constructed with explicit application-pinned verifying keys and a
minimum accepted catalog generation. Key rotation therefore requires an
application trust update. `GuestImageCatalogGenerationFloorV1` stores the
accepted generation under a stable, app-owned durable state directory, separate
from the disposable image cache. The signed first-install flow calls
`initialize_for_first_install` once with the app-release minimum; normal startup
must call `open` and treat missing, malformed, unsafe, or unwritable state as a
hard failure. It must never recreate a missing floor from cache, task input, or
an older catalog. Overlay launch takes this opened store and atomically advances
the floor after fresh manifest verification and before cache/QEMU use. The
directory and state files are owner-restricted, updates are locked and fsynced,
and a lower generation is rejected across process restart. The app must keep
this path stable in durable application data and provision it only from an
authenticated application release. The application embeds only the public key;
readiness uses it to check the bundled catalog, lock digest/generation, existing
durable generation floor, and cached image bytes. It opens existing state
without provisioning or repairing it. Missing floor or cache state remains
Unknown; readiness never initializes the anti-rollback floor.

The outer catalog uses camel-case JSON keys (`schemaVersion`,
`catalogGeneration`, `issuedAtUnixMs`, `expiresAtUnixMs`, `signerKeyId`,
`signatureHex`); image-entry keys use snake case (`image_digest`,
`artifact_size_bytes`, `source_revision`, `companion_protocol_version`).
Unknown fields are rejected. The x86_64 entry also fixes the artifact basename,
so the release fetch path cannot be supplied by a task or model. The
`computer-use-image-promotion` binary validates candidate sidecars, attestation
evidence, the source-built companion digest, and prior catalog state, then
signs `GuestImageManifestV1::signing_payload_bytes()`. Its output is verified
against the key embedded in this application before release. The private
signing key is only read from the protected promotion job's secret and is not
included in this tree or in the candidate build job.

Set the repository Actions secret `COMPUTER_USE_CATALOG_SIGNING_KEY` to the
32-byte Ed25519 seed as 64 hexadecimal characters (the optional
`ed25519-seed-hex:` prefix is accepted). The seed must derive to the public key
and key ID pinned by the application; the promotion workflow verifies the
signature against that pin before creating a release or review branch. Never
commit the seed or include it in logs, artifacts, issues, or pull requests. To
rotate it, generate and store a new seed in an approved secret manager, update
the application-pinned public key and key ID in a reviewed application release,
then update the repository Actions secret to the matching seed before promoting
a catalog.

The session binding is exact: session `image_digest` must equal an approved
catalog entry; `image_manifest_digest` is the canonical signed-manifest digest;
`image_signer_key_id` is the UUID catalog signer. The verifier also validates
the controller signature, current expiry/lease, and disposable session class
before returning `VerifiedGuestImageV1`. Expiry is exclusive:
`now >= expiresAtUnixMs` is expired. Missing key, stale generation, invalid
signature, revoked entry, changed digest, or mismatched session fails closed.

The only release download URL is derived from the verified release tag and the
fixed public GitHub repository. Redirects are HTTPS-only and host-allowlisted.
`ComputerImageDownloadApprovalV1` must approve the exact manifest/image digest
before the body is read. Setup can obtain a catalog-scoped download handle
before a VM session exists; that handle cannot create an overlay or start a VM.
The durable generation floor is checked and advanced before an approved
pre-session download, while readiness checks only read the floor. The cache
bounds size, checks the signed byte length
and SHA-256, writes a private temporary file, syncs then atomically installs
read-only content, and rehashes cached files on every use. There is no
user-controlled URL or tag fallback. Cache corruption fails closed for explicit
repair/re-download through
`remove_corrupt_for_approved_redownload`; it is never silently booted. The
replacement API again requires approval for the exact pair and removes only a
corrupt cache file. `GuestImageCacheV1::new` provisions the cache only for an
explicit operation. `open_existing` verifies an existing owner-private cache
without creating it or changing permissions, so readiness can remain
read-only. The verified cache handle
keeps the image digest, signed manifest digest, signer key ID, and catalog
generation available to the host integration as provenance evidence.

The guest has no app or user credentials and the candidate disables common
network-management services, but image contents alone cannot enforce the
runtime network boundary. The #380 host VM backend must attach no virtual NIC,
host home/repository mount, clipboard channel, device passthrough, or inherited
credential unless that capability is separately reviewed and authorized. The
v1 default is no network or host-shared resources.

`create_guest_image_overlay` requires fresh manifest bytes, current trust,
session attestation, and the opened durable generation floor. It rechecks
signature, expiry, revocation, session lease, architecture, session provenance,
and cached artifact digest at the overlay boundary. The caller cannot choose
the clock or an executable path: `probe_host_qemu_image_tool` resolves only the
fixed root-owned `/usr/bin/qemu-img`, and each invocation has a 20-second
timeout, bounded output capture, and process-group kill. It makes the base
read-only and creates an isolated qcow2 overlay. The overlay path is a random
UUID under a private directory. The VM backend must stop the VM before calling
`release_after_vm_stopped`. The local QEMU/KVM backend in #380 now connects to
these verifier/cache/floor APIs. The application must supply a stable durable
state path and perform authenticated first-install initialization before
launch; this image component does not select or create that application path.

## Trust and readiness dependencies

- #381 delivered the authenticated companion protocol, binary, and heartbeat;
  the signed generation-1 catalog binds the companion protocol version and
  binary digest. The #392 suite still owns evidence of a real boot and
  authenticated heartbeat; implementation or image promotion alone does not
  prove integrated guest readiness.
- #378 verifies promoted catalog and cached-image readiness with an
  application-pinned key. Do not add a private signing key or select from
  mutable tags.
- #392 owns real KVM boot, GUI capture/input, companion heartbeat, base
  immutability, two-overlay isolation, cleanup, and no-host-credential checks.
  Synthetic tests and build success do not establish those properties.
- CVE response: rebuild from a reviewed newer snapshot, compare package
  inventory and SBOM, run image and host acceptance, publish a new monotonically
  generated signed manifest, then revoke the affected catalog entry. Rollback
  is a new signed generation restoring a previously reviewed digest; generation
  rollback itself is rejected by the host trust policy.

## Test boundaries

Unit tests exercise strict catalog/session binding, stale-generation and
signature rejection, exact user approval before download, byte tampering, and
base/overlay metadata handling. They do not model QEMU's real qcow2 COW behavior
or boot a guest. The overlay command test substitutes a deterministic fake
`qemu-img`; it requires no Docker, network, SSH, or KVM and has low setup cost.
Real qcow2 backing-chain behavior, GUI startup, companion authentication, and
host isolation require the #392 real-KVM suite and are not claimed here.
