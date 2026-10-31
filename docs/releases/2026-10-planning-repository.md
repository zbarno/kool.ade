# Planning repository setup and recovery notes

The managed planning repository is the shared planning authority for a
workspace. Setup starts with a GitHub code repository and derives the paired
`<owner>/<repository>.packet` remote. Kool.ad/e clones both repositories; if
the planning repository is missing, it creates it with the code repository's
visibility. A visibility mismatch is reported without changing either repo.
Existing local workspaces reopen from their saved managed association.

## Embedded project data

Project artifacts embedded in a code checkout are unsupported. Kool.ad/e does
not discover, import, migrate, or use `.koolade-packet/` as a fallback. Existing
files remain untouched. Use the managed GitHub clone flow to establish the
code/planning pair; do not select a code or planning checkout by local path.

## Sync and limitations

Shared sync is an explicit Git operation against the configured planning
remote. Pending changes remain journaled when transport is unavailable;
permission failures, incompatible schemas, and record conflicts have visible
states and recovery paths. Operators should sync after joining and before
editing shared plans when possible. The application does not send real-time
change notifications. A conflict on one record requires an explicit local,
remote, or validated manual resolution. Git permissions govern the whole
repository; there is no per-field access control.

Code repository IDs, remotes, and task targets are shared. Checkout paths are
operator-local. Source and destination branches belong to the task's target
code repository, not to the planning repository. An operator without that
code checkout can review shared tasks but cannot execute tasks that require
the missing repository.

This setup does not rewrite Git history or import embedded project artifacts.
Verify the code and planning repositories are paired and that their remotes
have matching visibility before shared work begins.
