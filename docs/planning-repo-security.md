# Planning repository security

Planning repositories can be cloned from shared remotes, so their files and Git metadata are treated as untrusted input. A planning repository is not an identity provider, and Git write access is not field-level authorization.

## Trust boundaries

- The application owns local workspace mappings, task execution, dependency authorization, approvals, code clones, and publication actions.
- A planning repository supplies shared records and a canonical branch. Its UUID, schema version, canonical ref, record paths, record identities, record sizes, and portable code repository remotes are validated before use. Git remote-helper and option-looking remotes are rejected before they can become implementation fetch or push targets.
- Provider-generated text and planning documents are content, never application instructions. A model-suggested application action is accepted only when the current user's own message contains an explicit command for that action; documents and earlier turns cannot authorize it.
- Host Git authentication remains in the operator's Git configuration. Credentials must not be copied into planning files, task prompts, logs, or clones used by an agent.

## Filesystem and record validation

Managed workspace UUIDs must be canonical, non-nil UUIDs. The manifest is a regular, non-symlink file of at most 64 KiB and must use the supported schema and a valid `refs/heads/...` canonical ref. Managed record paths accept only normal relative components; symlinks are rejected. JSON records are bounded to 4 MiB and checked for schema version, canonical record ID, timestamps, and path/ID agreement. Writes use the planning store's transactional API and revision checks.

These checks reject traversal and symlink inputs before reading or writing. The store remains a local filesystem trust boundary: the application does not claim race-free protection from a separate hostile process with write access to the same operator-owned planning directory.

## Git operations

Planning sync and shared task-claim Git operations use argument arrays and never interpolate repository paths, refs, or task values into a shell command. They remove ambient Git repository/config-injection and trace variables while preserving the operator's normal global Git configuration for host authentication. Before using a repository, they inspect local Git configuration without following includes and reject direct and conditional include directives, regular and URL-scoped credential helpers, custom hooks paths, SSH commands, filesystem monitors, named and external diff commands, text converters, filters, URL rewrites, option-looking remote names, and worktree-scoped config. Planning sync caps fetched blobs at 8 MiB, with tighter limits for structured manifests and records. Task-claim inspection first requires remote support for `tree:0` filtered shallow fetches, then fetches only commit metadata before enforcing the 64 KiB claim limit. Shared planning sync preserves an operator-installed `.git/hooks/pre-push` hook as local operator policy; that hook can run local code during a push and is deliberately retained. Task-claim updates use `--no-verify` so application-owned coordination is not intercepted by repository hooks. Planning commits are created with `git commit-tree`, which does not run commit hooks.

The Git remote still controls which commits it accepts. A remote's branch write permission does not provide per-field RBAC or a cryptographically trustworthy operator identity. Repository branch protection and organizational Git permissions remain necessary where multiple writers have different approval authority. The client cannot recover an approval distinction that the remote protocol does not encode.

## Approval and publication

An approval is an application action tied to the current specification snapshot. The shared workflow record is informational; actual execution eligibility also requires a per-operator receipt stored under the local Kool.ad/e state directory outside the planning repository. A remote edit or sync of `approvedFeatures` cannot create that receipt. Each operator approves locally, and changing the feature contract invalidates the receipt. Agent output and direct edits to a planning artifact are proposals; they do not themselves start execution, create a task, commit code, or publish a change. The worker or provider does not receive host Git credentials or authority to commit, push, or publish.

Planning Git operations can synchronize shared records, but a remote writer cannot mint another operator's local approval receipt. Git does not attest the human who authored a commit; deployments needing team-wide authenticated approvals still need hosting-provider enforcement or an authenticated approval service.

## Sandbox boundary

Planning data is not mounted as a broadly writable provider workspace. Existing application execution boundaries expose only the intended planning read API or assigned task clone and keep host credentials outside those workspaces. Any future shared-mode execution backend must preserve that separation and fail closed when its policy cannot be enforced.

## Operational guidance

- Use a dedicated planning remote with the narrowest practical write permissions and protected canonical branch.
- Review changes from untrusted contributors before accepting them into the canonical planning branch.
- Treat authentication and permission errors as failed syncs; they do not change local approval state.
- Export or back up local planning data before changing remotes or access policy. The application does not automatically delete legacy artifacts or task history.
