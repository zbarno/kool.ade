# Intent

Packet is a native desktop application for planning software projects through conversation. It maintains a living specification, routes open questions and decisions, and organizes implementation stories and work in a Git repository.  Packet teases out ambiguity and drives clear focused specifications.

# Always

- Keep each source module under about 300 lines. Split growing modules into focused submodules before they become substantially larger.
- Use Rust's directory module layout: declare a module in `foo.rs` and place its child modules in `foo/` (for example, `core.rs` and `core/implementation.rs`).

# Never

- Never create or use a `mod.rs` file. Use the directory module layout described above instead.
