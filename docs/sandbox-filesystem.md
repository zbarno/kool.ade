# Docker execution filesystem boundary

Repository commands run in an application-owned Docker execution session. The
controller resolves the selected workspace and constructs a bounded staging
manifest; task text cannot supply host paths, mounts, image names, or Docker
settings. Read-only planning workspaces are mounted read-only. Implementation
uses an application-selected task clone, while its Git metadata is protected
from container writes. Returned files are checked against the staged baseline
before the controller applies them.

Containers use a read-only root filesystem, a non-root UID, dropped Linux
capabilities, `no-new-privileges`, private process and IPC namespaces, no
ambient network, and bounded CPU, memory, process count, temporary storage,
output, and deadline. Only the explicitly selected workspace is exposed. Host
home directories, provider credentials, Docker sockets, sibling projects, and
unrelated task clones are not mounted. Dependency and public-resource access is
mediated by application-owned brokers.

The provider CLI runs on the controller to preserve its account authentication
and model connection. Repository tools are disabled in the provider CLI and
provided through Kool.ad/e's private MCP bridge. This keeps provider credentials
out of the container; the provider-side host process remains part of the trusted
computing base. Locally visible host hooks or extra tools that could execute
outside the app-owned boundary are rejected where the provider exposes them to
inspection. Account-managed policies may not be fully inspectable, so operators
must trust those administrators and accounts.

Docker is a practical isolation layer, not a guarantee against every container
or host vulnerability. Docker daemon access and host mounts are powerful; the
application keeps the daemon connection in its trusted controller/worker and
never passes the Docker socket into the task container. See the [Docker
security matrix](docker-security-matrix.md) for test and release evidence.
