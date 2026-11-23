# Status

## Shipped

- Pipeline onboarding: `.sdlc/config.json`, the `sdlc` plugin and the PR
  review gate.
- TRU-9 (T-1 of TRU-8): the Cargo workspace; the server reads Linear every
  minute and serves `/v1/snapshot`, `/metrics` (`board_source_*`) and
  `/healthz`; the native app shows a column per stage with sub-tickets
  nested. Run it with `LINEAR_API_KEY=... cargo run -p board-server` and
  `cargo run -p board-app` (`BOARD_URL` defaults to `http://localhost:8080`).

## Next

- TRU-10: PRs, checks, gate results and the "Needs you" list.
- TRU-11: every repo, refresh on its own, stale sources.
- TRU-12: deploy, web build, Cloudflare Access. Then TRU-13 (Android) and
  TRU-14 (rollouts).
