# Application and repository data layout

This document summarizes the current architecture contract. The detailed
authority and publication rules live in the [storage boundary](koolade/architecture/storage-boundaries.md).

```text
project-code-repo/                         # one ordinary source Git repository
  src/
  docs/
    koolade/
      product/                             # published product modules
      features/                            # published feature specifications
      architecture/                        # durable architecture decisions

$KOOLADE_HOME/                             # private installation-local data root
  projects/<project-uuid>/
    operational-state/                    # tasks, drafts, approvals, conversations, evidence
    clones/                                # isolated task checkouts and execution metadata
```

Published specifications are ordinary Markdown documents in the designated
primary code repository. The default path for Kool.ad/e documents is
`docs/koolade/`; existing human documentation can remain elsewhere and be
referenced. Kool.ad/e must not overwrite existing documents during publication
preparation; changes are proposed through reviewed code-repository branches.

TaskIntents, child tasks, conversations, decisions, approvals, worker sessions,
execution leases, QA evidence, and history stay in local application storage.
This data is outside code repositories and task clones, is not tracked by Git,
and is not synchronized between installations. Local persistence uses durable
transactional writes and concurrency fencing.

The primary repository's designated published ref is resolved to an immutable
commit SHA before specifications are read. Manager and worker context records
that SHA and the document identity. A human checkout or task clone branch does
not change the published specification baseline.

## Explicit boundaries

- Kool.ad/e uses the code repository as the sole published-document authority;
  there is no paired planning repository or derived `<code-repository>.packet` remote.
- A `.koolade-packet/` directory in a code repository is never read, written,
  imported, or used as a fallback.
- Kool.ad/e does not create shared task records, remote task claims, or
  cross-installation task ownership.
- Specification drafts remain local until a separately authorized, validated
  publication prepares an isolated branch and normal pull request.
- External repositories and their history are outside local cleanup and are
  never deleted or rewritten by this architecture.
