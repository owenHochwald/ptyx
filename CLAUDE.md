# ptyx — Agent Quick Reference

ptyx is a Rust PTY proxy that improves SSH responsiveness via input buffering and echo prediction. It wraps the system `ssh` binary.

## Docs (load only what you need)
@docs/INDEX.md

## Rules
@.claude/rules/tdd.md
@.claude/rules/module-structure.md
@.claude/rules/async-safety.md
@.claude/rules/code-style.md
@.claude/rules/pr-checklist.md

## Common Commands
```bash
cargo test                          # all unit tests
cargo test --test '*'               # integration tests
cargo bench                         # benchmarks (release)
cargo clippy -- -D warnings         # must be zero warnings
cargo fmt --check                   # formatting gate
cargo run -- user@host              # run proxy
```
