# Docker Engine prerequisites

Repository commands use the application-owned Docker `ExecutionSession`. This
document describes the local engine and image checks made before a session is
created. A failed check denies repository tools; the application does not run
commands on the host as a fallback. The current application execution route is
Linux-only. Docker Desktop support on macOS and Windows is not production
verified.

## Supported local engine

- Native Linux must use rootless Docker Engine. Being in the `docker` group or
  running a task process as a non-root UID does not make a rootful daemon
  rootless.
- Docker Desktop on macOS or Windows is not enabled as a supported application
  execution target until path staging and cleanup are validated on those hosts.
- TCP, HTTP, SSH Docker endpoints, and unverified Docker contexts are rejected.
  Remote task execution will use the authenticated Kool.ad/e SSH worker; it will
  not forward a Docker API endpoint.
- Discovery checks daemon/API versions, Linux mode, architecture, rootless
  security options, seccomp, CPU/memory/PID limits, Docker image storage, and an
  application-owned workspace staging path. It performs no pulls, container
  creation, installation, daemon changes, or task execution.
- Readiness is cached for at most 15 seconds. Call `EngineDiscovery::invalidate`
  after a context switch or when a daemon restart is observed.

Failure is closed and categorized. Missing sockets, stopped daemons, permission
denials, rootful Linux, Windows containers, unsupported API/architecture,
missing limits/seccomp, insufficient disk, unknown profiles, and image digest or
platform mismatches do not fall back to host commands.

## Linux rootless setup boundary

The readiness checks below run after an Engine is available. The separate #346
setup controller has experimental rootless recipes for Debian 13, Ubuntu 24.04,
Pop!_OS 24.04, and Fedora 45 amd64. Each recipe uses a reviewed, consented
app-owned operation, an exact distro and package-manager match, pinned Docker
signing keys, and signed candidate-origin checks. Debian uses Docker's Trixie
APT source; Ubuntu uses Noble; Pop!_OS uses Docker's Ubuntu Noble source and
checks the Ubuntu archive or Pop!_OS Ubuntu mirror for helper packages. Fedora
uses Docker's exact Fedora 45 x86_64 stable DNF5 repository and separately
checks Docker, Fedora, and Updates package origins. Other releases and
architectures stay manual.

APT-family package scripts may enable or start the system rootful service, so
installation requires separate consent; Fedora's DNF package operation does
not activate that service. The app never stops, disables, or modifies it. The
operation can add the fixed subordinate-ID range when both current-user
mappings are absent, then create/start/enable Kool.ad/e's fixed current-user
systemd unit when it is safe to do so. Package and subid changes require a
native administrator prompt.

The unit writer only manages `~/.config/systemd/user`; a custom
`XDG_CONFIG_HOME` leaves automatic service setup manual.

If the source, signing key, package manager, user mappings, or service state does
not meet the exact recipe, the task remains manual. All listed recipes remain
experimental pending disposable-VM acceptance. Docker does not officially
support Ubuntu derivative distributions; Pop!_OS uses an explicit Noble
compatibility recipe and remains experimental until its VM acceptance passes.

The read-only readiness probe distinguishes missing, inaccessible, malformed,
and observed host facts. For automatic rootless repair it requires Linux kernel
4.15 or later, the documented minimum for Docker cgroup v2 support (see
[Docker's cgroup v2 requirements](https://docs.docker.com/engine/containers/runmetrics/#running-docker-on-cgroup-v2)).
It checks the kernel release, user-namespace controls,
subordinate UID/GID ranges, rootless helper executables, cgroup v2 controller
delegation, the current user's systemd runtime sockets, and the package manager
for the exact experimental recipe. Rootless Engine identity, security options,
resource limits, staging capacity, and image digests are checked separately
after connecting to an Engine.

On Linux, endpoint resolution derives `/run/user/<uid>/docker.sock` from the
current UID, so it does not depend on shell exports or a desktop launcher
forwarding `XDG_RUNTIME_DIR`. `DOCKER_CONTEXT=rootless` is accepted only when
its safely owned local metadata names that exact socket; other non-empty
contexts are rejected. `DOCKER_HOST` is accepted only when it names that exact
current-user socket.
The app persists a private, user-local selection record containing the current
UID, socket owner, and hashed endpoint and daemon fingerprints. It stores no
socket path, environment override, daemon ID, or credential. A changed UID,
socket owner, endpoint, or daemon invalidates the prior record; a matching
selection remains while a stopped daemon cannot be re-observed. Rootful
`/var/run/docker.sock` is not selected as a fallback. Untrusted sockets and
conflicting overrides fail closed.

The experimental Linux support matrix is Debian 13 amd64, Ubuntu 24.04 amd64,
Pop!_OS 24.04 amd64, and Fedora 45 x86_64. The app checks the exact
distro/version/architecture and package manager, then fails closed elsewhere.

## Manual setup and rollback

Until the listed recipes pass disposable-VM acceptance, use operator-led
setup and rollback. Consult Docker's official [rootless-mode prerequisites and
installation guide](https://docs.docker.com/engine/security/rootless/), [rootless systemd
user-service guidance](https://docs.docker.com/engine/security/rootless/tips/),
and [rootless troubleshooting and removal
instructions](https://docs.docker.com/engine/security/rootless/troubleshoot/).
Use Docker's [official Engine installation index](https://docs.docker.com/engine/install/)
to find the instructions for the exact distribution; links are reference
material, not a Kool.ad/e-verified distro recipe.

For a manual rootless setup, review each privileged package or host-configuration
change before applying it. Keep the rootless daemon under the current user's
service manager. Do not add the user to the `docker` group, broaden socket
permissions, expose a TCP daemon, or disable or restart a rootful service as an
automatic repair; any rootful-service change requires a separate operator
decision. The app's approved Engine package flow requires separate consent for
possible rootful service activation; if you install Docker Engine manually,
review its package-maintainer service effects. Docker documents that Engine
packages start the system-wide service by default on
Debian and Ubuntu. See
[Docker's post-installation service guidance](https://docs.docker.com/engine/install/linux-postinstall/#configure-docker-to-start-on-boot-with-systemd).
Verify the current-user socket and confirm that `docker info` reports the
rootless security option, then return to Kool.ad/e and recheck readiness. The
readiness scan is read-only and does not run a host-setup smoke test. Locked
image preparation has a separate approval and image smoke check. A rootless
report alone does not replace Kool.ad/e's Engine security, resource, workspace,
and approved-image checks. The rootless service remains awaiting verification
until the current socket, daemon, and approved image have a matching smoke
receipt.

### Remove the app-created user service

The app writes only
`$HOME/.config/systemd/user/koolade-rootless-docker.service`, and only when that
path is absent. An identical unit may have existed before setup and been reused;
the filename and contents alone do not prove that this operation created it.
The setup plan does not persist a per-file ownership marker. Remove the unit
only when an operator-held before/after record confirms it was absent before
this operation and the current file is still the app's unit. If that evidence
or ownership is uncertain, leave it in place for manual review.

Before stopping it, confirm no Kool.ad/e task or other workload is using this
rootless daemon. Check that
`systemctl --user show -p FragmentPath --value koolade-rootless-docker.service`
resolves to this exact path. Confirm that the unit file is a regular,
non-symlink file owned by the current user, and that its contents are exactly:

```ini
[Unit]
Description=Kool.ad/e rootless Docker service

[Service]
Environment=PATH=/usr/bin:/sbin:/usr/sbin:/bin
ExecStart=/usr/bin/dockerd-rootless.sh
Type=notify
Delegate=yes
Restart=always

[Install]
WantedBy=default.target
```

Also check `systemctl --user cat koolade-rootless-docker.service` to confirm
systemd loads that same file. If any check differs, stop and do not remove or
replace the file.

Once those checks pass, the bounded rollback is:

```sh
systemctl --user disable --now koolade-rootless-docker.service
rm -- "$HOME/.config/systemd/user/koolade-rootless-docker.service"
systemctl --user daemon-reload
```

These commands target only the current user's Kool.ad/e rootless unit. Do not
run system-level `systemctl stop` or `disable` for `docker.service` or
`docker.socket`; do not use Docker's rootless setup-tool uninstall command as a
substitute for removing this distinct app unit. Do not remove the containing
systemd directories or any Docker files under the user's home. Stopping and
removing the unit leaves Docker user data and configuration in place.

### Remove the app-created repository files

The recipe can create only these two APT files, and only when the relevant file
is absent:

- Signing key: `/etc/apt/keyrings/docker.asc`, whose approved Docker primary
  key fingerprint is `9DC858229FC7DD38854AE2D88D81803C0EBFCD88`.
- Debian 13 source: `/etc/apt/sources.list.d/koolade-docker.sources`, for
  `https://download.docker.com/linux/debian`, suite `trixie`, component
  `stable`, architecture `amd64`, signed by the key above.
- Ubuntu 24.04 and Pop!_OS 24.04 source: the same path, for
  `https://download.docker.com/linux/ubuntu`, suite `noble`, component
  `stable`, architecture `amd64`, signed by the key above.

The source file's exact content is:

```text
Types: deb
URIs: https://download.docker.com/linux/debian
Suites: trixie
Components: stable
Architectures: amd64
Signed-By: /etc/apt/keyrings/docker.asc
```

Ubuntu and Pop!_OS use the same template with `URIs:
https://download.docker.com/linux/ubuntu` and `Suites: noble`.

The app may reuse an existing valid key or an already configured exact Docker
source, so matching paths/content do not prove this operation created them.
Remove either file only if a before/after record confirms that it was absent
before this operation and created by it. The setup plan does not persist
per-file ownership markers; if that record is unavailable, preserve the files.
Before removal, confirm the source is a
root-owned regular non-symlink file with the exact content above, and the key is
a root-owned regular non-symlink file with the exact fingerprint above. Remove
the `koolade-docker.sources` file first. Remove `docker.asc` only after
confirming that no remaining APT source references
`/etc/apt/keyrings/docker.asc`. Never use a wildcard to remove Docker source
files, and do not remove the shared `/etc/apt/keyrings` directory; other
package sources may use it.

The Fedora 45 recipe can create only these RPM repository files, and only when
the relevant file is absent:

- Signing key: `/etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker`, pinned to Docker's
  primary key fingerprint `060A61C51B558A7F742B77AAC52FEB6B621E9F35`.
- DNF5 source: `/etc/yum.repos.d/koolade-docker-ce.repo`, for the Docker Fedora
  45 x86_64 stable repository with `gpgcheck=1`.

The repo file's exact content is:

```text
[koolade-docker-ce-stable]
name=Docker CE Stable - x86_64
baseurl=https://download.docker.com/linux/fedora/45/x86_64/stable
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-koolade-docker
```

Remove either DNF file only if an operator-held before/after record confirms it
was absent before the operation and created by it. Before removal, verify the
root-owned regular file, exact repository content or key fingerprint, and that
no other active source references the key. Remove the repo file first; keep the
key if another source uses it. Do not remove the containing directories.

### Package and user-data decisions

Package removal is a separate administrator decision, not part of service or
repository-file rollback. Debian, Ubuntu, and Pop!_OS request `docker-ce`,
`docker-ce-cli`, `containerd.io`, `docker-ce-rootless-extras`, `uidmap`, and
`dbus-user-session`. Fedora requests the four Docker packages plus
`shadow-utils` and `slirp4netns`. Some may have been installed before setup.
Use the package-manager history and recorded pre-setup state to identify which
packages, if any, this operation newly installed. Do not remove packages that
were already present. Before considering removal, inspect the system-level
`docker.service` and `docker.socket`. If either unit exists, is active or
enabled, or must remain available, retain `docker-ce`, `docker-ce-cli`, and
`containerd.io`; removing them can affect the rootful service. Any package
removal is a separate administrator operation. Preview only the confirmed
newly-installed package names with the matching package manager
(`apt-get -s remove` or `dnf remove --assumeno`) and proceed only if the operator
verifies that removal cannot stop, disable, remove, or otherwise alter the
system rootful service/socket or any unrelated package. If that cannot be
established, retain the packages. Do not use `purge` or `autoremove` as part of
this rollback. Never run service commands to stop or disable the system rootful
service or socket.

Removing the unit or repository files does not delete Docker user data or
configuration. Deleting rootless image/container data or user Docker
configuration is a separate operator decision; this rollback does not include
that deletion. If the relevant pre-setup state is unknown, preserve the data
and files and request manual review.

## Approved image profiles

`containers/images.lock.json` locks architecture-specific SHA-256 manifests
for `general`, `rust`, `node`, `python`, and `go`. Production selection accepts a profile and
the daemon architecture; it never accepts a repository or image name from task
text. Browser and desktop QA profiles remain disabled pending their separate
runtime lifecycle approval. Pulls are separate from health checks and can only
be requested through the bundled lock.
The runtime user is numeric UID/GID `10001:10001`.

Local implementation stages bounded task content into the container and
synchronizes validated changes back through the application-owned workspace
resolver. Git metadata remains separately protected. Docker-backed integration
tests assert the container's non-root identity, network isolation, resource
limits, host-secret denial, and task-clone synchronization. The CI gate runs
these tests against a pinned rootless Docker Engine.

## Saved task workspace compatibility

Task metadata remains separate from execution records. Legacy linked-worktree
tasks are migrated through a journaled workspace snapshot that preserves the
original worktree, branch, base commit, staged and unstaged changes, untracked
files, and symlinks. Interrupted migrations can resume only after validating
the saved snapshot and destination; conflicts leave both source and destination
available for review. An execution process is never resumed from serialized
command arguments. After a controller restart, unfinished Docker records are
marked interrupted by the execution recovery path, and a later task resume
creates a new execution attempt. See `src/core/implementation/execution/task_state/legacy_workspace.rs`,
`src/core/implementation/tests/task_repositories/legacy_migration.rs`, and
`src/execution/docker/backend/lifecycle/recovery/interrupt.rs`.

The operator's existing ignored `koolade/runtime-config.json` project grant
keeps its validated format. Local Docker execution may mount its approved
files read-only; remote execution requires a separate worker-bound grant. The
grant and file contents remain outside task commits and execution journals.
This avoids treating an old local grant as permission to transmit files to a
worker.

`containers/executor/Dockerfile` is a trusted recipe with a fixed build context.
It is never built from an agent repository. The `Trusted executor images` workflow
checks ten locked executor profile/platform combinations plus the two amd64 QA
image candidates on pull requests; after a change
reaches `main`, it publishes digest-addressed GHCR candidates with BuildKit SBOM and
SLSA provenance attestations and retains digest records as workflow artifacts. Review
the artifact and attestations before promoting any derived digest into
`containers/images.lock.json`. Runtime still uses the currently reviewed source
manifest pins until that promotion is committed; build tags are never trusted by the
runtime. See `containers/executor/README.md` for rebuild and promotion steps.

Docker Desktop's VM disk capacity cannot be inferred from the host's free space.
Until the engine reports a verifiable storage capacity, discovery fails closed
instead of treating host staging space as a VM capacity measurement.
