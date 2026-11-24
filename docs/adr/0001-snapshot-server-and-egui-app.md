# 0001: A snapshot server and one egui app

Status: accepted (TRU-8's plan, 2026-11-23)

The board shows Linear, GitHub and cluster state on Omarchy, Android and the
web, to one person, and no client may hold a Linear or GitHub token.

- One server on the cluster reads each source on an interval and serves the
  whole board as one JSON snapshot (`GET /v1/snapshot`). Reads stay far
  inside rate limits however many clients are open; the board is at most
  one interval plus one client refresh old.
- `crates/api` holds the snapshot types for the server and the app.
- One eframe app is built natively, for wasm (served by the server) and for
  Android, fetching with `ehttp`, which works on all three.
  - Amended by TRU-11: native builds fetch with `ureq` and a 20 s deadline
    on the whole call, because `ehttp`'s native GET has none before the
    response headers and a server that never answered froze the board. The
    wasm build still fetches with `ehttp` (the browser's fetch). The app's
    refresh logic (`board_app::Reads`) takes the time as an argument, so the
    wasm entry passes a browser-safe clock rather than `std::time::Instant::now`.
- Access is Cloudflare Access (a browser login, or a service token from the
  native and Android builds), and the server checks Access's signed header
  itself, so a request that skips Access gets nothing.
