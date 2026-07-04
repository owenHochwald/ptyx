# Repository Guidelines

## Project Structure & Module Organization

`ptyx` is a Rust 2021 Cargo project. Runtime code lives in `src/`: `main.rs` is the thin CLI entry point, `lib.rs` exports modules, and feature areas are split by concern (`proxy.rs`, `buffer.rs`, `predict.rs`, `pty.rs`, `terminal.rs`, `metrics.rs`, `config.rs`, `recorder.rs`, `replay.rs`, `display.rs`). Integration tests live in `tests/` and `tests/integration/`; Criterion benchmarks live in `benches/`. Architecture notes and subsystem references are in `docs/`; start with `docs/INDEX.md`.

## Build, Test, and Development Commands

- `cargo build` builds the debug binary and library.
- `cargo build --release` builds the optimized `ptyx` binary.
- `cargo run -- user@host` runs the proxy locally against the system `ssh`.
- `cargo test` runs unit and integration tests.
- `cargo test --test '*'` runs integration test targets explicitly.
- `cargo bench` runs Criterion benchmarks.
- `cargo fmt --check` verifies formatting.
- `cargo clippy -- -D warnings` enforces zero-warning linting.

## Coding Style & Naming Conventions

Use standard Rust formatting via `rustfmt` and keep modules focused on one responsibility. Public items require `///` doc comments. Prefer `anyhow::Result<T>` with contextual errors; do not use `.unwrap()` in production code. Use `tracing` macros for logging instead of `println!` or `eprintln!`. Naming follows Rust conventions: `PascalCase` types, `snake_case` functions and tests, and `SCREAMING_SNAKE_CASE` constants.

## Testing Guidelines

Follow TDD for behavior changes: write a failing test, run it, implement, then refactor. Put pure logic tests in `#[cfg(test)]` blocks beside the module. Use `tests/integration/<module>.rs` for real PTY or cross-module behavior. Name tests as behavior plus condition, for example `flush_triggered_when_deadline_expires`. Save and compare benchmark baselines for hot-path changes in `buffer.rs`, `predict.rs`, or `proxy.rs`.

## Commit & Pull Request Guidelines

Recent history uses short imperative subjects, and the project rule file specifies `type(module): short imperative description` for new commits, with types such as `feat`, `fix`, `test`, `bench`, `docs`, `refactor`, and `chore`. Before opening a PR, run `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and `cargo test --test '*'`. PRs should describe behavior changes, linked issues, tests run, and benchmark output when performance claims are made.

## Agent-Specific Instructions

Before touching a subsystem, read the relevant file in `docs/` and the applicable rules in `.claude/rules/`. Preserve async safety: no blocking I/O in async paths, no mutex guards held across `.await`, and never risk leaving the terminal in raw mode after a crash.
