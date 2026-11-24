# Status

## Shipped

- Pipeline onboarding: `.sdlc/config.json`, the `sdlc` plugin and the PR
  review gate.
- TRU-9: the server reads Linear every minute and serves `/v1/snapshot`,
  `/metrics` and `/healthz`; the native app shows a column per stage with
  sub-tickets nested. `LINEAR_API_KEY=... cargo run -p board-server`, then
  `cargo run -p board-app` (`BOARD_URL`, default `http://localhost:8080`).

## Next

- TRU-10 (PRs, gates, "Needs you"), TRU-11, TRU-12, then TRU-13 and TRU-14.
