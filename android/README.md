# The board's Android app

Kotlin and Compose on the board's Rust core (ADR 0002). `core` holds the
UniFFI bindings to `crates/ffi`, generated and committed; `app` is the app.

## Bindings

Their tests need only Rust and a JDK, and load the host build of `board-ffi`:

```sh
cargo build -p board-ffi && (cd android/core && ../gradlew test)
```

After a change to `crates/ffi` or a type it exports, regenerate and commit
them (CI fails on stale bindings):

```sh
cargo run -p board-ffi --features bindgen --bin uniffi-bindgen -- generate \
  target/debug/libboard_ffi.so --language kotlin --out-dir android/core/src/main/kotlin --no-format
```

## Build

CI's `android` job is the recipe: `cargo-ndk` builds `board-ffi` for
arm64 phones (the only ABI built) into `app/src/main/jniLibs`, then
`./gradlew :app:assembleRelease` gives an unsigned APK. The job signs it
with the keystore in the `ANDROID_KEYSTORE_BASE64` secret and its password
in `ANDROID_KEYSTORE_PASSWORD` (the key's too), and attaches it to the run
as `sdlc-board-apk`. Without the secrets the job fails, except on
Dependabot's PRs, which get none: a throwaway key signs those, and that APK
can't update an install signed with the real key. To build a branch's APK
before its PR, run the CI workflow on it.
