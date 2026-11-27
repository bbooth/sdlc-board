//! What both test files share.

/// Answers `get` with `FAKE_KEYS`, `rollout status` with exit `FAKE_ROLLOUT`
/// and `logs` with `FAKE_LOGS`; every call's arguments and stdin are kept in
/// `$REC/NN.args` and `$REC/NN.stdin`.
pub const FAKE: &str = r#"#!/bin/bash
n=$(printf '%02d' "$(ls "$REC" | grep -c args)")
printf '%s\n' "$@" > "$REC/$n.args"
cat > "$REC/$n.stdin"
case " $* " in
  *" get secret "*) printf '%b' "$FAKE_KEYS" ;;
  *" --type merge "*) if [ -n "$FAKE_PATCH_FAIL" ]; then cat "$REC/$n.stdin" >&2; exit 1; fi ;;
  *" rollout status "*) exit "${FAKE_ROLLOUT:-0}" ;;
  *" logs "*) printf '%s\n' "$FAKE_LOGS" ;;
esac
"#;
