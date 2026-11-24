# Status

## Shipped

- Pipeline onboarding: `.sdlc/config.json`, the `sdlc` plugin and the PR
  review gate.
- TRU-9: the server reads Linear every minute and serves `/v1/snapshot`,
  `/metrics` and `/healthz`; the native app shows a column per stage with
  sub-tickets nested. `LINEAR_API_KEY=... cargo run -p board-server`, then
  `cargo run -p board-app` (`BOARD_URL`, default `http://localhost:8080`).

- TRU-10: the server also reads each repo's open and recently merged PRs
  from its own branches on GitHub (checks, and gate records at the PR's
  head or merge) and gate output posted on Linear tickets. Cards show the
  latest gate and the PR; above the columns are failed sources and a "Needs
  you" list: specs to approve, green PRs to merge, blocked tickets. Set
  `GITHUB_TOKEN` (read-only): without one, a read sends at most 45
  requests and reads are spaced to keep to 45 of GitHub's 60 an hour
  (`GITHUB_ENDPOINT` to override). Gate records are read once per commit.
  A failed checks read keeps the last value for the same commit, or shows
  "checks unknown", and the source's age is that of the oldest data kept.

## Next

- TRU-11, TRU-12, then TRU-13 and TRU-14.
