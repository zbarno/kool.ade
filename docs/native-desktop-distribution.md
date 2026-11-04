# Native desktop packaging status

The native desktop workflow builds the primary `koolade` application on
GitHub-hosted Linux, macOS, and Windows runners. Each uploaded artifact is
unsigned, retained for 14 days, and tied to the source commit by a SHA-256
metadata file. Artifacts are CI evidence only. The workflow does not publish
GitHub Releases or change the supported-platform claims.

| OS and architecture | CI package | Release state | Runtime state |
| --- | --- | --- | --- |
| Linux x86_64 | Debian package | Source installer remains available; native package is an unsigned CI artifact | Current experimental host, Linux-only execution gates apply |
| Linux arm64 | Debian package | Unsigned CI artifact only | Not supported or advertised |
| macOS x86_64 | DMG containing a native app bundle | Unsigned CI artifact only; signing and notarization gate release | Desktop Docker transport and native validation remain incomplete |
| macOS arm64 | DMG containing a native app bundle | Unsigned CI artifact only; signing and notarization gate release | Desktop Docker transport and native validation remain incomplete |
| Windows x86_64 | Per-user Inno Setup installer | Unsigned CI artifact only; Authenticode signing gates release | Desktop Docker transport and native validation remain incomplete |
| Windows arm64 | None | Unsupported | Unsupported until native builds and host tests exist |

The Windows installer uses a per-user application directory and does not
remove application data during uninstall. The macOS bundle has a stable bundle
identifier, and the Linux package installs only the executable, desktop entry,
and icon. These packaging definitions have not yet been validated through
real-host install, launch to the Welcome screen, update, reinstall, or uninstall
cycles.

The application opens on the Welcome screen, which exposes Coding Tools and
the machine setup board before a workspace is connected. Native packages do
not register custom URL schemes, install providers, mutate the host, or bundle
provider credentials or a Docker daemon. Provider discovery must use bounded
direct process launch and explicit executable selection; shell startup hooks
are not run.

On first launch, setup records are initialized only in the current
installation-local data store. The application does not import or dual-read
older machine-setup locations. A real clean-install and restart check is still
required; no human-operated validation has been recorded for the greenfield
cutover.

## Release gate

There is no release publication job. Creating a release requires all of the
following first:

- Apple Developer signing identity and notarization credentials, with a
  verified signed and notarized DMG on both Mac architectures.
- Authenticode signing credentials and a verified signed Windows installer.
- An installed-package GUI launch that reaches the Welcome screen on each
  advertised host.
- Native-host installation, update, duplicate-install, uninstall, state
  preservation, launcher-path, Gatekeeper, Defender, and non-admin checks.
- Evidence that the advertised OS and architecture have supported execution
  boundaries. A successful native compile or package build is not runtime
  support evidence.

If signing credentials are absent, artifacts stay in CI and no user-facing
release is published. Linux x86_64 remains the only advertised desktop host.
Windows 11 x64 is the current Windows bootstrap target; Windows 10 and ARM64
remain unsupported until separate native end-to-end acceptance is recorded.
macOS, Windows, and Linux arm64 remain gated by the bootstrap support matrix.
