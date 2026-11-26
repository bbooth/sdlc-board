# sdlc-board

One board for all pipeline work: tickets by stage, PRs and their checks,
gate results and rollouts in flight, across every onboarded repo. A Rust
server on the homelab k3s cluster reads Linear, GitHub and the cluster and
serves one snapshot; an egui app shows it on Omarchy and the web, and a
Kotlin app on Android, all on one Rust core.
The first ticket is TRU-8 in Linear (project "SDLC board").

**Start with `docs/STATUS.md`**: what's shipped, what's next. Update it in
the same PR as the work.

## Planned layout

A Cargo workspace, not built yet:

- `crates/api/`: the snapshot types the server and the app share.
- `crates/server/`: the axum server that reads the sources and serves the
  snapshot and the web build.
- `crates/core/`: the board's logic every client shares: reads, settings,
  the native read and the wording.
- `crates/app/`: the egui (eframe) app, for native Linux and wasm; it only
  draws.
- `crates/ffi/`: `board-core` for Kotlin through UniFFI (ADR 0002).
- `android/`: the Android app (Kotlin, Compose). Its `core` module holds
  the generated bindings, committed: never hand-edit them; regenerate
  them as `android/README.md` says.
- `deploy/`, `argocd/`: the chart and Application, once it deploys.

## Pipeline

Work goes through the ticket pipeline from bbooth/agentic-sdlc (the `sdlc`
plugin, turned on in `.claude/settings.json`). Start a piece of work with
`spec-ticket`. `.sdlc/config.json` holds what the gates know about this
repo; `.sdlc/records/` holds the gate records.

- Keep Rust tests in files the config's `tests` patterns match
  (`crates/<crate>/tests/`, or a `tests.rs` / `*_tests.rs` module), not
  inline `#[cfg(test)] mod tests { ... }` blocks: the Build gate can't see
  inline tests.
- The owner signs off on the spec and the merge.
