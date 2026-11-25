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
  gate recorded last (so a ticket sent back shows its new gate, not the old
  failure) and the PR; above the columns are failed sources and a "Needs
  you" list: specs to approve, green PRs to merge, blocked tickets. Set
  `GITHUB_TOKEN` (read-only): without one, a read sends at most 45
  requests and reads are spaced to keep to 45 of GitHub's 60 an hour
  (`GITHUB_ENDPOINT` to override). Gate records are read once per commit,
  one file per stage, so after a start or a push a tokenless server can
  take a few reads to show every card's gate.
  A failed checks read keeps the last value for the same commit, or shows
  "checks unknown", and the source's age is that of the oldest data kept.

- TRU-11: `board.toml` lists booking and sdlc-board; adding a repo is a
  new `[[repos]]` entry (GitHub name, Linear team and project) and a
  restart. A ticket's PR comes from its own repo, and every repo's PR list
  is read before any PR's details. The app reads the snapshot every 30
  seconds and shows how old each source's data is; a failing source keeps
  its last data and is marked stale with its last good read and error. A
  failed app read keeps the board it had, and one the server never answers
  is given up on after 20 seconds. A PR's checks show within 2 minutes only with
  `GITHUB_TOKEN` set (GitHub is then read every minute): without one,
  GitHub reads are spaced by cost, often 20 minutes or more.

- TRU-12: the server checks Cloudflare Access's signed token
  (`Cf-Access-Jwt-Assertion`) on every route but `/healthz`, against
  `[access]` in `board.toml` (`issuer`, `aud`); `/metrics` moved to its own
  port, 9091. It serves the web build of the app at `/` from
  `BOARD_WEB_DIR` (`crates/app/web/build.sh` builds it). A local run
  without Access takes `BOARD_NO_ACCESS=1`. The native app sends a service
  token from `BOARD_ACCESS_CLIENT_ID` and `BOARD_ACCESS_CLIENT_SECRET`.
  `Dockerfile`, the chart in `deploy/`, `argocd/application.yaml` (one-time
  setup in `argocd/README.md`), and CI pushes `ghcr.io/bbooth/sdlc-board`
  from `main`.

- TRU-13: the Android build, `crates/android`, starts the same app in a
  native activity. On first start it asks for the board's address and an
  Access service token (client ID and secret) and keeps them in the app's
  own storage; Settings changes them. CI's `android` job builds a signed
  release APK, the run's `sdlc-board-apk` artifact, with the key in the
  `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD` secrets (a
  throwaway key without them). Locally, with `ANDROID_HOME` and
  `ANDROID_NDK_ROOT` set: `cargo install cargo-apk`, then
  `cargo apk build -p board-android` (debug key) or `--release` with
  `CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`.
  The native binary is now `crates/app/src/bin/board-app.rs`.

## Next

- TRU-14 (rollouts).
