---
name: project-phase4-complete
description: Phase 4 (Config File + Session Recording) was implemented and all tests pass
metadata:
  type: project
---

Phase 4 is complete as of 2026-05-27. All 138 tests pass.

**Why:** Phase 4 added TOML config file support, session recording, and replay subcommand.

**How to apply:** The project is in post-Phase-4 state. The only remaining work in TODO.md is the "Ongoing" section (cargo deny check, README updates, pitfalls review before PRs).

New modules added in Phase 4:
- `src/recorder.rs` — `SessionRecorder` writes `.ptyx` logs to `~/.local/share/ptyx/sessions/`
- `src/replay.rs` — `parse_session()` + `replay_session()` for `ptyx replay <file>`
- `src/config.rs` — Added `FileConfig`, `ProxyFileConfig`, `DisplayFileConfig`, `BackendConfig`, `RunMode` enum

Key design decisions:
- CLI args use `Option<T>` to distinguish "explicitly set" from "default"; file config fills gaps
- Boolean flags (--adaptive, --predict, etc.) are OR'd: CLI OR file → enabled
- Session log format: text lines, `PTYX v1 <unix_ts>` header, then `I/O <us> <hex>` events
- Replay caps inter-event gaps at 2s to keep playback snappy
- `--record` flag enables recording; off by default
