# Computer Use guest image catalog and cache (CU-03)

Status: host-side verifier/cache/overlay APIs and a candidate build recipe are
implemented. There is no promoted image, app-pinned catalog release key, or
CU-05 companion binary yet. Images are not operationally ready: the guest unit
fails closed until the authenticated companion reports readiness. Real KVM GUI,
companion, and overlay validation belongs to #392.

## Image build boundary

`containers/computer-use/build-image.sh` is an app-owned build recipe. It uses a
fixed Debian Bookworm snapshot and installs a graphical Openbox/LightDM/Chromium
guest, UTC locale/time settings, and a locked non-root `koolade` account. It
accepts only an output directory; it does not inspect task repositories,
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
promotion workflow, or Docker images. This repo currently has no production
catalog signing key or CU-05 companion payload, so the workflow must not publish
a signed catalog or label the candidate trusted. The component inventory does
not claim OS-file coverage or license compliance.
`containers/computer-use/images.lock.json` is an explicit empty VM lock with
`status: not_promoted`; it is separate from `containers/images.lock.json` and
must remain empty until a real, signed catalog can be pinned.

The service unit `/etc/systemd/system/koolade-computer-use-agent.service` is
installed but its executable is intentionally absent. The service is
`Type=notify`; no heartbeat can be emitted until #381 supplies the authenticated
guest companion. A future manifest can carry companion protocol version and
binary SHA-256, but those values must describe the exact reviewed package; the
synthetic unit-test value is not a release value.

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
authenticated application release. No key is embedded in this feature branch.

The outer catalog uses camel-case JSON keys (`schemaVersion`,
`catalogGeneration`, `issuedAtUnixMs`, `expiresAtUnixMs`, `signerKeyId`,
`signatureHex`); image-entry keys use snake case (`image_digest`,
`artifact_size_bytes`, `source_revision`, `companion_protocol_version`).
Unknown fields are rejected. The x86_64 entry also fixes the artifact basename,
so the release fetch path cannot be supplied by a task or model. Offline
promotion tooling signs `GuestImageManifestV1::signing_payload_bytes()` with
the release key held outside the repository; no signing key belongs in this
tree or in the image build job.

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
before the body is read. The cache bounds size, checks the signed byte length
and SHA-256, writes a private temporary file, syncs then atomically installs
read-only content, and rehashes cached files on every use. There is no
user-controlled URL or tag fallback. Cache corruption fails closed for explicit
repair/re-download through
`remove_corrupt_for_approved_redownload`; it is never silently booted. The
replacement API again requires approval for the exact pair and removes only a
corrupt cache file. The verified cache handle
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
`release_after_vm_stopped`. These APIs are not yet connected to a production VM
backend; #380 consumes the verifier/cache/floor interface. The application
must supply a stable durable state path and perform authenticated first-install
initialization before launch; this component does not select or create that
application path.

## Trust and readiness dependencies

- #381 owns the authenticated companion protocol, binary, and actual heartbeat.
  Until it lands and is cryptographically bound in a signed catalog, the guest
  must be reported unavailable even if the desktop is visible.
- #378 consumes `GuestImageManifestTrustV1` and
  `verify_guest_image_manifest`; its app trust integration must supply keys
  pinned by the signed application distribution. Do not add a private signing
  key or select from mutable tags.
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
