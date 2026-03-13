# Mentra

Mentra is an agent runtime for building tool-using LLM applications.

This repository is a small workspace:

- `mentra/`: the publishable runtime crate
- `examples/`: example programs built on top of the runtime

Consumer-facing crate docs live in [mentra/README.md](mentra/README.md).

If you want the packaged crates.io quickstart, install the published example:

```bash
cargo install mentra --example quickstart
OPENAI_API_KEY=... quickstart "Summarize the benefits of tool-using agents."
```

## Workspace Commands

Run the richer interactive workspace example after cloning the repository:

```bash
cargo run -p mentra-examples --example chat
```

Run checks:

```bash
cargo check --workspace
cargo test --workspace
```
