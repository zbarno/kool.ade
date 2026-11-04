# Clean CLI launch profiles

Kool.ad/e starts external providers from a private controller working directory
and passes arguments as discrete values. Repository and command access stays
behind the application execution boundary. Each validated launch binds the
selected executable and version evidence, `ExecutionProfile` (`Manager` or
`Worker`), generated argv, working directory, and filtered environment before
the process is spawned. The child environment retains only protected runtime
lookup/locale values, provider authentication variables, and values generated
for the current run.

## Profile readiness and role eligibility

Discovery records provider-specific Manager and Worker profile IDs in
`HarnessCapabilitySnapshotV1`, alongside the exact CLI version and current
authentication-source class (`Environment` or `ProviderProfile`). A blocked
profile has no eligibility ID. Unknown or changed auth-source or profile
evidence fails closed and requests an automatic refresh. At launch,
the selected profile is included in the validated launch seal and checked
against provider-specific blockers. This evidence establishes the app's
configuration and eligibility decision; it is not a vendor attestation or
proof of authenticated-provider behavior.

| Provider | Manager profile | Worker profile | Current profile-specific controls and limits |
| --- | --- | --- | --- |
| Pi | Conditional | Conditional | The only currently eligible Manager profile. Both disable sessions, context files, extensions, ambient skills, templates, themes, and built-in tools; only app MCP tools are exposed. Bounded Worker implementation may additionally load the single app-owned `koolade-task` skill. No subagents are enabled. |
| Codex CLI | Blocked | Blocked | Private `CODEX_HOME`, ephemeral JSON execution, ignored user config, read-only sandbox profile, and isolated saved auth handling are configured. Account-managed hooks/policy cannot be preflighted or contained, so neither role may launch. |
| Claude Code | Blocked | Blocked | Private MCP configuration, restricted mode, no native tools, no session persistence, and local managed-setting checks are configured. Server-managed hooks/policy cannot be preflighted or contained, so neither role may launch. Claude is also unavailable under WSL because Windows-managed policy cannot be inspected. |
| OpenCode | Blocked | Conditional | Private config/home, deny-by-default permissions, app MCP allowlist, and project-config/external-skill flags are set. Their effective behavior for project instructions and plugins has not been verified against the installed CLI, so Manager launch is blocked. |
| GitHub Copilot CLI | Blocked | Conditional | Private home/cache and explicit app MCP allowlist are used; built-in MCPs, custom instructions, remote access, and remote export are disabled. Managed instruction/extension behavior is not verified for strict Manager use. |
| Antigravity (`agy`) | Blocked | Conditional | Private HOME/XDG, strict tool settings, workspace-only access, terminal sandbox, and app MCP allowlist. Manager is blocked because suppression of ambient instructions/managed policy is not verified; Worker remains conditional on current evidence and mode prerequisites. |

“Conditional” requires a fresh role/mode capability snapshot, installed CLI
version, authentication, model, scriptable invocation, and (for tool modes) the
app bridge plus required execution target. It does not mean every provider or
account is currently ready. Pi is the only currently eligible Manager profile.
Codex and Claude are fail-closed for both profiles; Antigravity, OpenCode, and
Copilot are fail-closed for Manager. Routing shows the blocker, refreshes
stale evidence automatically, and does not silently substitute a provider.

The Manager receives app-supplied context and mode-limited read-only MCP tools;
it does not receive repository project-guidance files as executable
instructions. `AGENTS.md`, worker output, and repository content remain
untrusted evidence. The application owns action validation, approvals, QA,
publication, merge, and deployment. Repository operations use the app-owned
Docker execution boundary, and provider credentials are not mounted into the
Docker worker.

The only native worker skill currently enabled is the explicit app-owned Pi
`koolade-task` skill for bounded implementation. It is generated in private
launch scratch and passed as one explicit skill while ambient skill discovery
remains off. No provider-native subagent support is enabled or claimed. Worker
features may be expanded only with separate evidence that their effects remain
inside the task clone, Docker/host, credential, network, MCP, review, and
approval boundaries.

## Provider invocation and environment

The application probes each installed CLI's bounded `--help` output in a
temporary HOME/XDG tree before declaring it ready. It rejects a provider whose
help output is missing a required option. Probed options are cached for the
executable path, reported version, and executable modification time. The probe
proves that the installed binary advertises the arguments used by the profile;
mocked tests prove the application passes them. It does not prove every
behavior of a provider binary, so authenticated real-provider smoke remains a
release follow-up.

Role-profile evidence is bound to the probed version and current authentication
source class and expires with discovery freshness. Before a routed call, the
application checks that the evidence is still current and asks the CLI for its
version again. Providers with managed account hooks or policy that cannot be
preflighted are blocked for the affected roles; a class-level auth check does
not claim to detect every same-class account change.

The controller-side provider environment rebuilds `PATH` from OS-protected
directories only. On Unix, every path component must be root-owned and not
writable by group or other users; on Windows, only the system root and its
`System32` directory are retained. Relative/empty entries, user-owned paths,
and paths resolving inside the task checkout or private launch directory are
removed, including symlinks into those roots. The provider executable itself
is launched by its validated absolute path, so user-managed CLI install
directories do not need to be added to child `PATH`.

Exact tokens and environment keys are defined in
`src/harness/execution_security/application_boundary/providers.rs`. Runtime
configuration is in the matching provider modules under
`src/harness/execution_security/application_boundary/providers/`.

Project guidance is separate from ambient CLI instructions. Where supported,
it is entered and reviewed in Project Settings; saving changed text requires an
explicit approval checkbox, and the private receipt binds the exact content
hash and project identity. Selected prompts may include the approved snapshot
as user-provided data. The Manager mode does not load that repository guidance;
neither it nor an ordinary `AGENTS.md` can grant authority.

## Role and CI coverage

`fake_provider_invocations_cover_all_modes_and_launch_profiles` exercises the
fake provider executables across all current execution modes. It checks the
filtered environment, generated values, isolated controller working
directory, and that MCP authority matches mode `ToolAccess`. Application
boundary tests also verify no-access behavior, profile argv/environment
binding, role-specific blockers, and rejection of missing capabilities or
post-validation drift. The invocation matrix places a synthetic command in the
task checkout's `bin` directory and verifies the spawned controller process
cannot find or run it through inherited `PATH`.

The hosted `tests and Docker runtime` job runs these tests in parallel with the
complete all-target nextest suite. A focused profile test step runs capability,
binding, and role-matrix tests first. No paid provider credentials or installed
vendor CLIs are required by this gate.

The Unix-only
`every_provider_role_calls_only_its_application_owned_mcp_authority` test
checks that tool-enabled provider configs register only the app-owned MCP
server. The fake CLI uses the parsed executable and config path with the fixed
internal-server flag to invoke it, then sends permitted requests to a
controlled socket fixture. Roles without tools must register no MCP server.
The test also verifies read-only tool denial and session identity for
tool-enabled roles. The fixture records requests without executing commands;
it does not prove vendor CLI behavior or Docker execution.

## Telemetry and security boundary

Task activity stores provider name, normalized CLI version, execution-profile
ID, selected prompt role version when available, a SHA-256 profile identity,
and the capability names accepted by the probe. Codex also records whether
authentication came from the environment, a saved profile, or is unavailable;
other providers do not record an auth strategy. This is compatibility evidence,
not an attestation from the provider or a replacement for the Docker execution
boundary. Telemetry omits argv, environment values, paths, prompts, and
credentials. Provider API credentials are available only to the controller-side
CLI process; they are never added to the Docker worker environment.

## Upstream provider documentation

- [Pi coding agent usage and CLI options](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/usage.md)
- [Codex CLI reference](https://developers.openai.com/codex/cli/reference)
- [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference)
- [OpenCode CLI](https://opencode.ai/docs/cli/)
- [GitHub Copilot CLI command reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)
- [Antigravity headless CLI](https://www.antigravity.google/docs/cli/headless/)

## Required follow-up before claiming full provider equivalence

- Run authenticated-provider smoke coverage for each supported strict Manager
  profile. Record provider/version/auth-source class, role, effective
  instructions and tools, cancellation, and observed result. This is deferred
  integrated release validation; synthetic CI is mock-only, and this document
  makes no real-provider behavior claim.
- Verify that each provider's supported flags actually suppress user and
  project customization sources; help output alone cannot establish semantics.
- Extend auth-strategy telemetry to other providers only when their selected
  auth source can be identified without recording secrets. Codex auth-source
  telemetry is implemented. Add role/task identifiers only when those values
  can be obtained without recording secrets.
- The mock-process integration test proves concurrent same-provider tasks get
  separate private OpenCode homes/configs and cancellation kills only the
  selected process tree. It captures `OPENCODE_CONFIG` and
  `OPENCODE_CONFIG_DIR`, reads each generated config, and checks each config
  contains only that task's app server. It does not establish vendor session
  cancellation or cleanup semantics.
- Test same-provider task cancellation and private-state cleanup against
  installed provider processes before claiming real-provider equivalence.
