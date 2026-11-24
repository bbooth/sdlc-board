# sdlc-board

One board for all pipeline work: tickets by stage, PRs and their checks, gate results and rollouts in flight.

## Running it

- Server: `LINEAR_API_KEY=... BOARD_NO_ACCESS=1 cargo run -p board-server`
  (settings in `board.toml`; `docs/STATUS.md` lists the rest).
- Native app: `BOARD_URL=https://board.trulinc.com
  BOARD_ACCESS_CLIENT_ID=... BOARD_ACCESS_CLIENT_SECRET=... cargo run -p board-app`,
  with the Cloudflare Access service token (`argocd/README.md`).
- Web build: `crates/app/web/build.sh dist`, then the server with
  `BOARD_WEB_DIR=dist` serves it at `/`.
