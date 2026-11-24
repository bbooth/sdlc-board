# Status

## Shipped

- Pipeline onboarding: `.sdlc/config.json`, the `sdlc` plugin and the PR
  review gate.
- TRU-9: the server reads Linear every minute and serves `/v1/snapshot`,
  `/metrics` and `/healthz`; the native app shows a column per stage with
  sub-tickets nested. `LINEAR_API_KEY=... cargo run -p board-server`, then
  `cargo run -p board-app` (`BOARD_URL`, default `http://localhost:8080`).

- TRU-10: the server also reads each repo's open PRs from GitHub (checks,
  and gate records on the PR's branch) and gate output posted on Linear
  tickets. Cards show the latest gate and the PR; a "Needs you" list above
  the columns has specs to approve, green PRs to merge and blocked tickets.
  Set `GITHUB_TOKEN` (read-only): without one, GitHub's limit of 60
  requests an hour runs out within the hour (`GITHUB_ENDPOINT` to override).

## Next

- TRU-11, TRU-12, then TRU-13 and TRU-14.
