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

- TRU-14: the server also reads the Argo Rollouts in each repo's
  `rollout_namespace` (`board.toml`; `rolloutNamespace` in the chart's
  values, booking's is `booking`) every minute, through the pod's
  ServiceAccount, which may only get and list Rollouts there
  (`deploy/templates/rbac.yaml`). A canary whose new version isn't stable
  yet shows on the card of each of its repo's tickets being rolled out
  (Stage Rollout, or merged and still in PR review): its step, weight,
  phase and analysis. Images are tagged `latest`, so two tickets merged
  close together both show it. A local run reads `KUBE_ENDPOINT` (e.g.
  `kubectl proxy`) without a token; without it, or a cluster, rollouts
  aren't read.

- TRU-18 (part of TRU-17): the server can sign in to GitHub as a
  read-only GitHub App, so private repos' checks show. Set
  `GITHUB_APP_ID` and `GITHUB_APP_PRIVATE_KEY` (the App's PEM key); it
  then reads every minute with a read-only token scoped to each repo,
  replaced in its last five minutes (the old one serves until it expires
  if a new one can't be got). A reinstalled App or a revoked token
  recovers on the next read, without a restart. A repo the App isn't
  installed on fails alone ("the GitHub App isn't installed on ..."). One
  setting without the other, or a key that doesn't parse, stops the server
  at start. With the App set, `GITHUB_TOKEN` is ignored with a warning;
  without it, the token or tokenless reads work as before.
  `board_github_app_tokens_total{result}` counts the tokens got, and every
  failure to get one, a failed installation lookup included. TRU-19:
  the chart passes both settings from `sdlc-board-tokens` (optional), and
  `argocd/README.md` step 4 is the App's one-time setup.

- TRU-28 (TRU-13's first task): the board's logic is one crate,
  `crates/core` (`board-core`): when to read (`Reads`), the settings and
  their Access headers, the native read with a User-Agent naming the
  client and an error naming why a read failed, and how every line on a
  card is worded (`words`). The egui app only draws. The server counts
  requests by `client` (`android`, `native`, `web` or `other`, from the
  User-Agent) in `board_http_requests_total`.

- TRU-29: `crates/ffi` (`board-ffi`) exports the core to Kotlin with
  UniFFI (ADR 0002): settings trimmed and checked, a `Board` that reads
  when due and keeps the last board, and the board as already-worded
  lines. `android/` is the Gradle project: `core` holds the committed
  bindings and their JVM tests, `app` only shows the core's version so
  far. CI's `android` job checks the bindings are fresh, builds
  `board-ffi` for arm64 phones, runs the tests and attaches a signed APK
  (`sdlc-board-apk`). `android/README.md` says how to build it.

## Next

- TRU-13 (Android): TRU-30 (settings and reads), TRU-31 (the board
  screen).
