# Decisions

| Decision | Current rule |
| --- | --- |
| Published documentation authority | The primary code Git repository at a pinned commit SHA. |
| Task and runtime authority | Private local application storage outside the code repository. |
| Plan adoption | A user explicitly adopts a recommendation before it unlocks approval. |
| Specification publication | The user reviews the exact documentation change before a protected pull request is opened. |
| Task implementation | Each task uses an isolated clone with local concurrency fencing. |
