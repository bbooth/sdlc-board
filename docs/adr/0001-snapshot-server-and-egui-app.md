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
    response headers and a server that never answered froze the board.
  - Amended by TRU-12: the web build fetches with the browser's `fetch`
    through `web-sys`, with an `AbortSignal` timeout for the same deadline,
    since `ehttp`'s web fetch takes no timeout. Each fetch is behind a
    `target_arch` cfg, so `ureq` is native-only. The app's refresh logic (`board_app::Reads`)
    keeps time with `web_time::Instant`, which is std's natively and the
    browser's clock on wasm, where `std::time::Instant::now` panics.
  - Amended by TRU-13: Android runs the app from its own crate,
    `crates/android`, in a `NativeActivity` packaged by `cargo-apk`, and
    fetches with `ureq` like the native build. Android builds eframe
    without its `accesskit` feature, which doesn't build with the native
    activity. `cargo-apk` is no longer developed; it needs no Java or
    Gradle, and the APK has nothing but the library. If it stops building,
    `xbuild` or a Gradle wrapper around `cargo-ndk` replaces it. It targets
    SDK 34: at 35 Android draws the app edge to edge, and a NativeActivity
    can't read the window insets to keep clear of the system bars. Moving
    to 35 means GameActivity (Gradle) or reading the insets over JNI.
- Access is Cloudflare Access (a browser login, or a service token from the
  native and Android builds), and the server checks Access's signed header
  itself, so a request that skips Access gets nothing.
  - Amended by TRU-12: every route but `/healthz` checks it; `/metrics` is
    served on its own port, which no route reaches, so Prometheus needn't
    pass Access. The server won't start without `[access]` settings unless
    `BOARD_NO_ACCESS=1`, for local runs.
- The cluster is read with the same `reqwest` client as Linear and GitHub,
  one GET per namespace, rather than the `kube` crate the plan named
  (amended by TRU-14): one list call didn't need a Kubernetes client and
  its dependencies.
