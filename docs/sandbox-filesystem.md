# Linux sandbox filesystem boundary

Planning and implementation start with an empty Bubblewrap root. A read-only
bind still permits disclosure of everything beneath it; making the host root
read-only is not a confidentiality boundary. Neither profile mounts `/`, `/usr`,
`/usr/local`, `/usr/share`, `/etc`, `/opt`, or `/var` as a host directory.

## Trusted system runtime

Both profiles use `src/harness/pi_sandbox/mounts/system_runtime.rs`:

- Executables: `/usr/bin`, `/usr/sbin`, `/usr/local/bin`, `/usr/local/sbin`.
- Libraries and helpers: the `lib`, `lib64`, and `libexec` directories directly
  under `/usr` and `/usr/local`.
- Build headers: `/usr/include` and `/usr/local/include`.
- Shared runtime data: only `locale`, `zoneinfo`, `nodejs`, `npm`,
  `ca-certificates`, `git-core`, `perl`, `perl5`, `cmake`, `autoconf`, `aclocal`,
  and `libtool` directly under `/usr/share`. Perl and build-tool data is also
  permitted at the matching `/usr/local/share` paths. CMake, Automake, and
  aclocal version directories are discovered only by exact program prefixes
  followed by dot-separated numeric versions.
- Compatibility paths: `/bin`, `/sbin`, `/lib`, `/lib64`, resolved to the
  same approved runtime directories on merged-layout Linux distributions.
- Individual OS files: `/etc/ld.so.cache`, `/etc/passwd`, `/etc/group`,
  `/etc/nsswitch.conf`, `/etc/localtime`; Fedora's `nsswitch.conf` may come
  from the single file `/etc/authselect/nsswitch.conf`. Public certificate
  roots include `/etc/ssl/certs` and Fedora's
  `/etc/pki/{tls/certs,ca-trust/extracted/pem,ca-trust/extracted/openssl}`.
- Compiler and build-tool alternatives: only `cc`, `c++`, `cpp`, `automake`,
  and `aclocal`. Canonical symlinks preserve executable lookup and GCC's
  support-file lookup; the alternatives directory remains hidden.

Each runtime source is canonicalized before binding. A redirect outside the
allowlist, wrong file type, or dangling symlink blocks sandbox construction
with an environment-prerequisite diagnostic. Optional absent runtime paths
are omitted. Local-time redirects are limited to timezone data; certificate
directory redirects are limited to the public certificate roots.

These executable/library directories are trusted host installations. Private
files stored inside them are readable. Keep credentials, customer data, and
host configuration out of installed runtime directories. Symlinks *inside*
read-only directories cannot make hidden destinations visible, because those
destinations have no host mounts. Repository content does not extend the list.

## Project and tool-specific access

Planning mounts only its selected project and registered related projects,
read-only. Implementation mounts its assigned task repository read-write and
protects Git metadata separately. Broad host roots, host-home ancestors,
protected system trees, and paths overlapping runtime directories are rejected
as project roots. A project can live in a dedicated data directory such as
`/usr/local/src/project` without exposing its neighbors.
Credential-directory components and Git administration roots are rejected,
as are project roots within `/run`, `/var/lib`, and `/var/log`.

Node and .NET installation detection checks the same runtime visibility list.
An SDK elsewhere uses the existing narrowly validated installation mount;
location anywhere under `/usr` no longer implies that it is visible.
Planning's external Node installation support is unchanged: its fixed runtime
PATH supports the system Node installation. Custom Node locations newly hidden
by this allowlist may require a supported system installation for planning;
implementation uses the separate Node installation mount.
Rust toolchains, package caches, Pi's installation, provider/resource sockets,
and explicitly granted project `.env` files use their separate mount policies.
Private project configuration remains subject to the existing grant and
no-fresh-download restrictions. These are explicit exceptions, not a blanket
grant of host-home access.

Both profiles clear inherited environment variables, isolate network and
process namespaces, create private temporary directories, and mount private
`/proc` and `/dev` views. Provider and dependency brokers run outside the
sandbox and grant only their supported operations. Missing build tools must
be handled as environment prerequisites, never by restoring a broad host bind.

## Validation and remaining scope

Tests cover mount-policy construction, runtime redirects into private data,
dangling links, broad project roots, and read denial for unrelated host paths
in both profiles. Ubuntu Linux CI runs the complete C, Cargo, npm, credential,
network, and Git-metadata fixtures. Fedora 44 CI runs the runtime-layout policy
tests against Fedora's authselect and certificate paths.

Pi is currently the only coding CLI with an implementation route. Kool.ad/e
declares that route through a typed execution policy covering filesystem scope,
network, tools, dependency authorization, credentials, process isolation, and
supported modes. Other coding CLIs remain discoverable, but the application
rejects their repository routes before starting a process. OpenCode command
filters are not treated as a security boundary. Trusted system runtime mutation
by another host process during construction is outside this boundary; the
sandbox does not make an already compromised host trustworthy.
