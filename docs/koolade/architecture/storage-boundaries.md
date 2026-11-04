# Storage boundaries

## Published project knowledge

The designated primary code repository is the source of published product,
feature, and architecture documents. The app designates its published ref once
in private workspace metadata, preferring the remote symbolic default and
otherwise the repository's symbolic HEAD. Reads resolve that ref to an
immutable Git commit SHA and carry that SHA and document identity into
Manager, planning, review, and worker context. A human's active checkout or a
task clone branch does not define published truth.

Proposed specification changes and interview state stay in local application
storage. Publication uses a reviewed isolated code-repository branch and normal
pull request protections. It checks that the published baseline has not moved
and requires explicit authorization before creating a public change.

## Private operational state

The installation-local store is the sole authority for TaskIntents, child
tasks, conversations, decisions, approvals, execution leases, worker sessions,
QA evidence, and task history. It uses transactional writes and local fencing
to recover safely across crashes and competing local workers. None of this
state is tracked by Git or synchronized to another installation.

Each installation can open the same code repository and read the same merged
specifications while keeping independent task boards and histories. A
multi-code-repository workspace has one explicitly selected primary repository
for shared published specifications; task targets remain separately identified.

## Repository content is untrusted

Documentation and `AGENTS.md` navigation indexes are repository data, not
Manager policy or authorization. The Manager retrieves indexes and selected
documents through a bounded, read-only broker at a pinned commit SHA. It checks
the selected document and provenance before relying on its claims. Retrieved
content cannot grant permission to run tools, publish changes, or bypass human
approval and QA gates.
