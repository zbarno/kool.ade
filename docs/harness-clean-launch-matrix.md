# CLI clean-launch matrix (current candidate)

The launcher removes the same inherited startup-code, shell-init, Git-config,
loader, and IDE-connection variables from all six provider processes and their
capability probes. It leaves provider authentication and model-transport values
(such as API keys and endpoint variables) available to the controller-side CLI;
none are added to the Docker worker environment. The denylist is centralized in
`UNSAFE_PROVIDER_LAUNCH_ENV` and tested for the shared spawn path.

This is a bounded hardening increment, not proof that every installed provider
version meets issue #156. Except for Pi's runtime help-capability check, current
provider versions and several vendor-managed configuration surfaces are not
version-gated. A provider must remain unavailable when those surfaces can enable
host execution outside the application boundary.

| Provider | Current clean-launch controls | Authentication and model transport | Remaining ambient-policy limit |
| --- | --- | --- | --- |
| Pi | JSON print mode; no session, context files, extensions, skills, prompt templates, or themes. A private per-run Pi agent, session, and package directory is used. Implementation disables built-in tools and enables only the app extension. Required flags are checked against installed `--help`. | A private profile copies only the selected provider/model settings and that provider's auth entry; provider/model are passed explicitly. The profile files are created with owner-only permissions. `PI_OFFLINE=1` and `PI_SKIP_VERSION_CHECK=1` limit startup behavior. | Pi planning reads and implementation use the common Docker MCP bridge and typed execution session. Real installed-version coverage remains required for every supported flag and capability. Pi capability checks verify option presence, not behavioral support by a vendor version; trust and package-discovery behavior still needs real installed-version coverage. |
| Codex | Private controller cwd; `--ignore-user-config`; read-only native sandbox; native shell, hooks, plugins, memories, skills, code mode and other listed features disabled; system and user hook files inspected. Readiness now requires the `--disable`, `--ignore-user-config`, and `--skip-git-repo-check` controls in addition to base exec options. | Only `OPENAI_API_KEY` and explicit model transport survive the clean child environment. Host `CODEX_HOME` and `~/.codex/auth.json` are not imported; the current readiness probe checks host login state, so subscription/file-auth readiness does not prove isolated-task authentication. | Version detection checks required option presence, not behavioral support for every feature name. Host OAuth/auth-file and OS-keyring compatibility needs a dedicated controller-side auth strategy; account-managed policies cannot be inspected. |
| Claude Code | Private controller cwd; restricted mode, no native tools or permission prompts, strict app-only MCP config, no session persistence; local managed hooks/MCP rejected. | `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` remain on the host controller process. Host `~/.claude` credentials are not imported; `auth status` currently probes the host profile, so OAuth readiness does not prove isolated-task authentication. | User-level configuration and cloud-managed policy are not fully isolated; WSL is rejected because Windows-managed settings cannot be inspected. |
| OpenCode | `--pure`; private controller cwd and generated config; native tools denied; generated MCP only; auto-update, default plugins, LSP downloads and Claude integration disabled. | Imports selected provider/model settings and auth into private controller state. | Imported model/provider data is a compatibility bridge; no vendor-version floor is currently enforced. |
| GitHub Copilot CLI | Private `COPILOT_HOME` and cache; `--no-custom-instructions`; built-in MCPs, remote access and export disabled; available tools restricted to app MCP. | Imports a supported saved GitHub token into private controller state; token environment variables remain available. | Plugin/skill/hook/custom-agent discovery and enterprise policy are not fully version-gated. |
| Antigravity (`agy`) | Stream-JSON headless mode; private HOME/XDG and generated strict settings; native file, command, URL and unsandboxed tools denied; app MCP is the only allowed tool. | Only `GEMINI_API_KEY` is retained as a supported API-key variable; `GOOGLE_API_KEY` is dropped because the CLI documents that it is ignored. The CLI requires explicit `modelProvider: "gemini"` alongside the key, which the current profile does not select. | API-key mode cannot be inferred safely from key presence because that could silently change billing. Private HOME/keyring OAuth behavior and vendor-managed settings need installed-version smoke coverage; no compatible-version gate exists yet. |

The CLI references used for the controls include [Pi usage](https://github.com/earendil-works/pi-mono/blob/main/packages/coding-agent/docs/usage.md), [OpenAI Codex CLI](https://developers.openai.com/codex/cli/reference), [Claude Code CLI](https://code.claude.com/docs/en/cli-reference), [OpenCode CLI](https://docs.opencode.ai/docs/cli/), [GitHub Copilot CLI](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference), and [Antigravity headless mode](https://www.antigravity.google/docs/cli/headless/). Confirm flag behavior against the installed version before expanding these profiles.

The launch process runs from a private bootstrap directory; repo tools are provided
through the existing application bridge. Task clones remain unchanged. Provider
API traffic remains controller-side and is outside the repository-command
sandbox; disabling native repo tools does not make provider inference offline.
