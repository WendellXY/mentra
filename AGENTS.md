# AGENTS

## Philosophy: First Principles

Agents must reason from first principles. Do not rely on conventions, copied patterns, or assumptions without verification. Every task should begin by identifying the fundamental facts, constraints, and invariants of the system (e.g., API contracts, type rules, data models, performance limits). Decompose problems until they reach irreducible components, then derive solutions logically from those facts. Prefer the simplest design that satisfies all constraints, and explicitly verify assumptions using available evidence (code, documentation, tests, or tools). Avoid guesswork, pattern imitation, or speculative implementations. Solutions should be the result of facts → constraints → reasoning → implementation.

## Workflow Discipline

- Commit each completed step before starting the next step. Do not batch multiple distinct steps into one uncommitted working state.

## Commit Style

- Use Conventional Commits for every commit message.
- Format commit subjects as `<type>(<scope>): <summary>`.
- Use one of these types: `feat`, `fix`, `docs`, `refactor`, `chore`, `test`.
- Prefer narrow, concrete scopes that match the actual files or feature area being changed, such as `figma`, `parser`, `tests`, or `docs`.
- Avoid generic scopes like `core` when they do not name a real, specific area of this repository.
- Write summaries in the imperative mood and describe the change, not the activity.

## Rust Programming

When working in Rust in this repository, follow the language's current common style and prefer the standard Cargo toolchain over ad hoc commands.

- Prefer `foo.rs` plus `foo/` over `foo/mod.rs`. If module `foo` has child modules, keep the parent in `foo.rs` and place children under `foo/`. Do not create both `foo.rs` and `foo/mod.rs`.
- Prefer current edition idioms over legacy patterns. Avoid `extern crate` unless a specific compatibility constraint requires it.
- Let `rustfmt` define formatting. Run `cargo fmt` after Rust edits instead of hand-formatting to a custom style.
- Use `cargo check` for fast compiler feedback while iterating.
- Use `cargo test` for behavior verification. Prefer targeted tests while iterating, but run the full relevant verification command before claiming completion.
- Use `cargo clippy --all-targets --all-features -- -D warnings` when practical for lint-clean changes.
- Keep modules focused. Split large files by responsibility rather than accumulating unrelated types and functions in one place.
- Prefer explicit types and derives when they capture invariants clearly, and use the type system to model domain constraints instead of relying on comments or unchecked conventions.

