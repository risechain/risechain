# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Engineering Principles

- **Fundamental**: Seek deep understanding of the problem space and underlying concepts. Make decisions from first principles, not patterns or assumptions.
- **Correctness**: Never compromise correctness even if it results in less elegant or slower code. Always preserve invariants and safety.
- **Simplicity**: Seek the most obvious solutions. Avoid clever abstractions, unnecessary layers, or overengineering.
- **Clarity**: Obvious code needs no comment, but non-obvious code must have comments, even in paragraphs if necessary. A module should be understandable on its own without requiring deep reading of its dependencies and related modules.
- **Minimalism**: Do exactly what is required for the task. Avoid extra features and speculative generalization. Keep code and diffs the bare minimum. However, do not compromise simplicity; if structurally better and easier to follow, longer code is fine.
- **Performance**: Milliseconds matter, so write performance-aware code. Avoid unnecessary computations, channels, data serialization, and especially external calls. Reuse caches and allocations where possible, and don't pay for what we don't use.
- **Observability**: Emit meaningful logs and metrics on critical paths for ease of debugging. However, do not overdo it and spam noise.

## Commands

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --workspace --all-targets --all-features --locked

# Build
cargo build --all-targets --all-features --locked
```

`RUSTFLAGS=-Dwarnings` is enforced in CI — all warnings are errors.
