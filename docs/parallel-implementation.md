# Parallel implementation and integration

The task's declared affected files are a scheduling heuristic. They help avoid
starting obviously overlapping local tasks at the same time, but declarations
can be incomplete. Local execution leases fence competing workers in one
installation. Separate installations do not share task state or coordinate
claims; they exchange code only through ordinary Git operations.

Before integration, Kool.ad/e derives each verified task's changed paths from
NUL-delimited Git name-status records. It compares that set with destination
changes since the task base, including rename sources and destinations,
deletions, binary files, symlinks, and file-directory prefix conflicts. It
fetches and pins the destination commit before this comparison. Conflicting
integration runs in an isolated clone; unresolved changes preserve both task
and integration clones and require review. The final push uses non-force and
lease checks so a destination update after preflight cannot be overwritten.

Git change-set checks protect integration when independently performed work
touches overlapping paths. Integration runs in an isolated clone, preserves
unresolved conflicts for review, and uses non-force pushes with lease checks so
a destination update cannot be overwritten.
