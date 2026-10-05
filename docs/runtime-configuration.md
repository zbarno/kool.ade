# Local configuration for verification

A task runs in an isolated Git worktree. Git does not transfer ignored `.env`
files from the normal checkout. You can explicitly grant a project `.env` so
Kool.ad/e mounts the existing file read-only at the same relative task path.
Changes to that file take effect when the next sandbox starts.

The private grant lives at `.git/koolade/runtime-config.json` in the normal
checkout. Give the `koolade` directory mode `0700` and the file mode `0600`,
owned by the repository owner. Use this schema:

```json
{
  "schema_version": 1,
  "source_root": "<canonical absolute path of the normal checkout>",
  "files": ["App/.env"]
}
```

`source_root` must be the normal checkout's canonical path. Each entry must be
an exact relative filename ending in `.env`, ignored and untracked in both
checkouts. Symlinks, paths outside the checkout, duplicate entries, tracked
files, and missing source files are rejected. Up to 32 files of 1 MiB each are
supported. The file must stay ignored through reconciliation of both histories.
An existing task copy is left untouched and hidden by the read-only mount.
An absent task copy may acquire an empty mount target; secret values are never
copied into the task checkout.

Granted configuration is excluded from automatic recovery stashes and task
commits. If an older recovery retained configuration whose grant is removed,
resume stops for review instead of putting that file into a stash.

The shell remains offline. Worker resource downloads are disabled whenever
private project configuration is granted, so dependency caches must already be
available. Application-managed refresh of public NuGet audit data remains
available. Do not print configuration values in logs or agent reports.

After updating the grant, resume the task. A malformed grant fails closed and
preserves the worktree for review. To revoke access, remove the entry; review any
previous recovery record before resuming a task that retained that file.
