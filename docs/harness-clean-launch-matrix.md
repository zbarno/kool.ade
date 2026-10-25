# Clean CLI launch profiles

Kool.ad/e starts providers from a private controller working directory, passes
argv as discrete values, and keeps task filesystem/process access behind the
application execution boundary. The launch profile binds the selected
executable, version evidence, generated argv, working directory, and filtered
environment before the process is created. The child environment retains only
runtime lookup/locale values, provider-specific API authentication variables,
and values generated for the current run.

## Provider matrix

The application probes each installed CLI's bounded `--help` output in a
temporary HOME/XDG tree before declaring it ready. It rejects a provider whose
help output is missing a required option. Probed options are cached only for
the executable path, reported version, and executable modification time. The
probe proves that the installed binary advertises the arguments used by the
profile; mocked tests prove the application passes them. It does not prove
every behavior of a provider binary, so real-provider smoke tests remain a
release follow-up.

The controller-side provider environment rebuilds `PATH` from OS-protected
directories only. On Unix, every path component must be root-owned and not
writable by group or other users; on Windows, only the system root and its
`System32` directory are retained. Relative/empty entries, user-owned paths,
and paths resolving inside the task checkout or private launch directory are
removed, including symlinks into those roots. The provider executable itself
is launched by its validated absolute path, so user-managed CLI install
directories do not need to be added to child `PATH`.

| Provider | Runtime-checked profile controls | App-owned setup | Authentication and remaining limits |
| --- | --- | --- | --- |
| Pi | print/JSON mode, no session, no context files, no extension/skill/template/theme discovery, no built-in tools, explicit tool list/extension, selected provider/model/thinking level | `PI_CODING_AGENT_DIR`, package/session directories, explicit Kool.ad/e extension, offline mode | API auth variables remain controller-side. A real installed Pi smoke test is not part of mandatory CI. |
| Codex | JSON exec, read-only sandbox, ephemeral session, explicit model/config, ignored user config, skipped Git repository check, explicit feature disables | Private MCP config and only the application-owned tool server | `OPENAI_API_KEY` and an explicitly provided `CODEX_ACCESS_TOKEN` are passed to the controller-side Codex process. Saved subscription login import and project-instruction suppression need real CLI verification. |
| Claude Code | prompt/stream JSON, no session persistence, restricted mode, no native tools, no permission prompts, explicit permission mode and strict generated MCP config | Private MCP config and explicit app bridge allowlist | API key/auth token variables remain controller-side. Machine-managed settings that can run host code are rejected. Subscription authentication and project instruction suppression need real CLI verification. |
| OpenCode | pure mode, JSON event output, automatic tool mode, explicit directory/agent/model | Generated config, private OpenCode HOME/config/cache, no default plugins, explicit deny-by-default permissions and MCP | Known API-key variables remain controller-side; saved OpenCode auth is copied only into the private home. Real provider startup is not part of mandatory CI. |
| GitHub Copilot CLI | silent prompt mode, no ask-user, no auto-update, explicit tool allowlist and MCP config, built-in MCPs/custom instructions/remote access/export disabled | Private `COPILOT_HOME` and cache, generated MCP config | Only Copilot GitHub token variables are passed, with saved token import limited to supported token formats. Host machine policy hooks from filesystem and Windows registry are rejected. |
| Antigravity (`agy`) | headless input/output stream JSON, explicit model/effort support, terminal sandbox | Private HOME/XDG and generated strict settings/MCP config | Only `GEMINI_API_KEY` is passed as an API credential. Subscription login and the provider's full project-discovery behavior need real CLI verification. |

Exact tokens and environment keys are defined in
`src/harness/execution_security/application_boundary/providers.rs`. Runtime
configuration is in the matching provider modules under
`src/harness/execution_security/application_boundary/providers/`.

## Role and CI coverage

`fake_provider_invocations_cover_all_modes_and_launch_profiles` launches all
six mocked provider executables in every `ExecutionMode`. It checks the
filtered environment, generated values, isolated controller working
directory, and that MCP authority matches the mode's `ToolAccess`. The
application-boundary tests also verify no-access behavior for each provider,
profile argv/environment binding, and rejection of missing capabilities or
post-validation drift. The six-provider invocation matrix additionally places
a synthetic command in the task checkout's `bin` directory and verifies that
the spawned controller process cannot find or run it through inherited `PATH`.

The hosted `tests and Docker runtime` job runs these tests in parallel with the
complete all-target nextest suite. A focused profile test step runs the
capability, binding, and role-matrix tests first so the clean-launch release
gate is visible independently in CI. No paid provider credentials or installed
vendor CLIs are required by this gate.

The Unix-only `every_provider_role_calls_only_its_application_owned_mcp_authority`
test checks that tool-enabled provider configs register only the app-owned MCP
server. The fake CLI uses the parsed executable and config path with the fixed
internal-server flag to invoke it, then sends permitted requests to a controlled
socket fixture. Roles without tools must register no MCP server. The test also
verifies read-only tool denial and session identity for tool-enabled roles. The
fixture records requests without executing commands; it does not prove vendor
CLI behavior or Docker execution.

## Telemetry and security boundary

Task activity stores the provider name, normalized CLI version, selected prompt
role version when available, a SHA-256 profile identity, and the capability
names accepted by the probe. The record omits argv, environment values, paths,
prompts, and credentials. It is compatibility evidence, not an attestation from
the provider or a replacement for the Docker execution boundary. API
credentials are only available to the controller-side CLI process; they are
never added to the Docker worker environment.

## Upstream provider documentation

- [Pi coding agent usage and CLI options](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/usage.md)
- [Codex CLI reference](https://developers.openai.com/codex/cli/reference)
- [Claude Code CLI reference](https://code.claude.com/docs/en/cli-reference)
- [OpenCode CLI](https://opencode.ai/docs/cli/)
- [GitHub Copilot CLI command reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference)
- [Antigravity headless CLI](https://www.antigravity.google/docs/cli/headless/)

## Required follow-up before claiming full provider equivalence

- Run opt-in smoke jobs with supported real CLI versions and test authentication,
  startup, stream completion, model selection, and bridge calls.
- Verify that each provider's supported flags actually suppress all user and
  project customization sources; help output alone cannot establish semantics.
- Add provider-specific auth-strategy and role/task identifiers to telemetry
  only when those values can be obtained without recording secrets.
- A mock-process integration test proves concurrent same-provider tasks receive
  separate private OpenCode homes/configs and that cancellation kills only the
  selected process tree. It captures the child's `OPENCODE_CONFIG` and
  `OPENCODE_CONFIG_DIR`, reads each generated config file, and verifies that
  each generated config contains only that task's app server. It does not establish vendor
  session cancellation or cleanup semantics.
- Test same-provider task cancellation and private-state cleanup against
  installed provider processes before claiming real-provider equivalence.
