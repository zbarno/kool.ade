## 9. Environment, Launch, and Preconditions

Hard prerequisites: Linux x86_64 workstation, Rust toolchain, system `git`, writable planning-root repository, and an operator-provisioned Pi CLI. Multi-repository projects additionally need each target checkout mapped locally and its Git remote identity matching `.planner/project.json`. PR mode needs `gh` authentication; Auto mode needs push rights on the target remote. Git remote fetch/push may require network access (D-30 supersedes the old no-refresh observation CLR-019).

Pi discovery follows `PACKET_PI_BIN`, `PATH`, then common user install directories; its version is displayed without a minimum pin (D-12, D-13, D-15, F-16). `PACKET_TURN_TIMEOUT_SECS` is a positive integer read at turn start, default twelve hours (D-24 supersedes D-08). `PACKET_HOME` changes private state location. `HOME` must be resolvable for normal persistence.

Identity is a soft precondition: git `user.name`, then `user.email`, then configured fallback, then `(guest)` (D-14, D-23, FR-13). This project's explicit owner is recorded under D-25. An absent identity degrades routing but does not prevent connection.

Launch from source with `cargo run --offline`. A new feature requires explicit in-app approval before task generation or Auto execution. Connected repositories with legacy `planning/specification.md` migrate on connection; existing task snapshots remain historical. The deferred collaboration channel does not participate in launch (D-11, D-18, D-22; CLR-001 resolved).
