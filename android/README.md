# The board's Android app

Kotlin and Compose on the board's Rust core (ADR 0002). `core` holds the
UniFFI bindings to `crates/ffi`, generated and committed; `app` is the
Android app.

## Test the bindings

On any machine with Rust and a JDK (no Android SDK needed):

```sh
cargo build -p board-ffi
cd android/core && ../gradlew test
```

The tests load `target/debug/libboard_ffi.so`, the host build.

## Regenerate the bindings

After any change to `crates/ffi` (or a type it exports), from the repo root:

```sh
cargo build -p board-ffi
cargo run -p board-ffi --features bindgen --bin uniffi-bindgen -- \
  generate target/debug/libboard_ffi.so --language kotlin \
  --out-dir android/core/src/main/kotlin --no-format
```

Commit the result. CI fails if the committed bindings are stale.

## Build

The app needs the Android SDK, the NDK and `cargo-ndk`
(`cargo install --locked cargo-ndk --version 4.1.2`), and the
`aarch64-linux-android` Rust target. From the repo root:

```sh
ANDROID_NDK_HOME=<ndk> cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build -p board-ffi --release
cd android && ./gradlew :core:test :app:assembleRelease
```

That gives an unsigned `app/build/outputs/apk/release/app-release-unsigned.apk`.
Only arm64 phones are built for.

## Release key

CI's `android` job signs the APK with the keystore in the repo's
`ANDROID_KEYSTORE_BASE64` secret and its password in
`ANDROID_KEYSTORE_PASSWORD` (the key's password is the same), and
attaches it to the run as `sdlc-board-apk`. Without the secrets the job
fails, except on Dependabot's PRs (which get no secrets): a throwaway key
signs those, and that APK can't update an install signed with the real key.

To build an APK from a branch before its PR, run the CI workflow on that
branch (Actions, CI, Run workflow).
