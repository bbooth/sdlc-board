# 0002: A native Android app on the shared Rust core, through UniFFI

Status: accepted (TRU-13's plan, 2026-11-26). Supersedes ADR 0001's Android line.

The egui build of the app ran on Android, but a phone couldn't paste into
its fields (so the Access secret had to be typed), and it drew under the
system bars. The owner chose a native Kotlin and Compose app instead, with
the board's logic kept in Rust and shared with the desktop and web builds.

- `crates/core` (`board-core`, from TRU-28) stays the one copy of the
  logic: when to read, what to keep after a failed read, the settings and
  their Access headers, the native read, and how each line is worded.
- `crates/ffi` (`board-ffi`) exports it with UniFFI's proc-macros. Its
  `Board` reads when due and returns the board as plain records whose
  lines are already worded, so Kotlin only lays them out and the phone
  says what the desktop says. Sub-tickets come flattened, each with its
  depth, rather than as a recursive record.
- The Kotlin bindings are generated from the built library and committed
  in `android/core`, and CI fails if they're stale, as `gen/` works in
  the other repos. Committed, a Kotlin change needs no Rust toolchain to
  read and review.
- `android/core` is a plain Kotlin/JVM build of its own (as lab-notebook's
  `core/api` is), so the bindings' tests run on the JVM against the host
  build of `board-ffi`, without the Android SDK or an emulator. It only
  compiles against JNA; the app brings JNA's Android build.
- `board-ffi` is built for `arm64-v8a` only, the owner's phone, with
  `cargo-ndk`. CI signs the APK with `apksigner` and the release key, so
  the build needs no key alias.
- lab-notebook's ADR 0033 plans the same shape for `notebook-core`.

Rejected: staying on egui and working around its Android text input,
and a Kotlin rewrite of the logic (two copies of the wording and the read
rules to keep in step).
