# Parallel implementation and integration

The task's declared affected files are a scheduling heuristic. They help avoid
starting obviously overlapping tasks at the same time, but declarations can be
incomplete and local in-flight state does not coordinate separate app
instances.

Before integration, Kool.ad/e derives each verified task's changed paths from
NUL-delimited Git name-status records. It compares that set with destination
changes since the task base, including rename sources and destinations,
deletions, binary files, symlinks, and file-directory prefix conflicts. It
fetches and pins the destination commit before this comparison. Conflicting
integration runs in an isolated clone; unresolved changes preserve both task
and integration clones and require review. The final push uses non-force and
lease checks so a destination update after preflight cannot be overwritten.

Cross-instance task claims coordinate the same task UID. They do not serialize
different task UIDs; Git change-set checks protect integration when those tasks
touch overlapping paths. Claim takeovers retain the prior owner and session in
the remote claim record.
