# Planning repository cutover notes

The managed planning repository is the shared planning authority for a
workspace. New workspaces have one planning Git repository and zero or more
code repositories. A workspace can be created without a code checkout, and
operators join by connecting to its planning repository. Joining keeps the
workspace UUID and task identities and records local checkout mappings only
for the operator who configures them.

## Existing data and migration

Managed planning repositories use normalized per-record files at the
repository root, as specified in [Planning repository v1](../planning-repository-v1.md).
Legacy `.koolade-packet/` and recognized historical planning layouts remain
migration inputs. Migration preflights sources and destinations, keeps
conflicting copies, preserves recognized IDs and approval fingerprints, and
retains the original checkout and monolithic source files as recovery
evidence. Missing code checkouts do not block planning migration. No legacy
source is automatically deleted.

To roll back, stop planning edits, preserve the managed planning repository
and operator-local workspace association, and reconnect to the preserved
legacy checkout. If managed planning has received newer shared edits, export
or manually reconcile those changes before resuming work from the legacy
copy; the application does not automatically reverse-sync normalized records
into legacy monolithic files. Keep a backup of both repositories before any
manual recovery.

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

This cutover does not rewrite Git history, remove embedded artifacts, or
claim that separate PR merges are integrated until their commits are
verified in the target repositories. Keep the legacy checkout until the
managed workspace has been opened, synced, and its required records checked.
