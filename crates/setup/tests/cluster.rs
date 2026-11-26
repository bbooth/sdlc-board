//! `board-setup key` against a fake kubectl that records every call and
//! plays back the cluster's answers.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const KEY: &str = include_str!("../../server/tests/fixtures/app_test_key_pkcs1.pem");
const SIGNED_IN: &str = r#"{"fields":{"message":"reading GitHub as the GitHub App","app_id":42}}"#;

/// Answers `get` with `FAKE_KEYS`, `rollout status` with exit `FAKE_ROLLOUT`
/// and `logs` with `FAKE_LOGS`; every call's arguments and stdin are kept in
/// `$REC/NN.args` and `$REC/NN.stdin`.
const FAKE: &str = r#"#!/bin/bash
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

struct Run {
    code: i32,
    out: String,
    calls: Vec<(Vec<String>, String)>,
    dir: tempfile::TempDir,
}

impl Run {
    /// The calls whose arguments include every one of `words`.
    fn find(&self, words: &[&str]) -> Vec<&(Vec<String>, String)> {
        self.calls.iter().filter(|(args, _)| words.iter().all(|w| args.iter().any(|a| a == w))).collect()
    }

    fn patches(&self) -> Vec<&(Vec<String>, String)> {
        self.find(&["patch", "secret", "sdlc-board-tokens"])
    }

    fn restarts(&self) -> usize {
        self.find(&["rollout", "restart"]).len()
    }
}

fn run(key: &str, keys: &str, rollout: i32, logs: &str) -> Run {
    run_with(key, keys, rollout, logs, &[])
}

fn run_with(key: &str, keys: &str, rollout: i32, logs: &str, env: &[(&str, &str)]) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let (rec, work) = (dir.path().join("rec"), dir.path().join("work"));
    fs::create_dir_all(&rec).unwrap();
    fs::create_dir_all(&work).unwrap();
    let fake = dir.path().join("kubectl");
    fs::write(&fake, FAKE).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_board-setup"))
        .args(["key", "--app-id", "42"])
        .current_dir(&work)
        .env("TMPDIR", &work)
        .env("KUBECTL", &fake)
        .env("REC", &rec)
        .env("FAKE_KEYS", keys)
        .env("FAKE_ROLLOUT", rollout.to_string())
        .env("FAKE_LOGS", logs)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(key.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let mut names: Vec<PathBuf> = fs::read_dir(&rec).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    let calls = names
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "args"))
        .map(|p| {
            let args = fs::read_to_string(p).unwrap().lines().map(String::from).collect();
            (args, fs::read_to_string(p.with_extension("stdin")).unwrap_or_default())
        })
        .collect();
    Run { code: output.status.code().unwrap_or(-1), out, calls, dir }
}

/// The key's body lines: any one of them showing means the key leaked.
fn key_lines() -> Vec<&'static str> {
    KEY.lines().filter(|l| !l.starts_with("-----") && l.len() > 16).collect()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            found.extend(files_under(&p));
        } else {
            found.push(p);
        }
    }
    found
}

/// AC-5: the key is never in an argument, the output or a file the run leaves.
fn assert_no_leak(r: &Run) {
    for line in key_lines() {
        assert!(!r.out.contains(line), "the key is in the output:\n{}", r.out);
        for (args, _) in &r.calls {
            assert!(!args.iter().any(|a| a.contains(line)), "the key is in an argument: {args:?}");
        }
        for f in files_under(&r.dir.path().join("work")) {
            assert!(!fs::read_to_string(&f).unwrap_or_default().contains(line), "the key is in {}", f.display());
        }
    }
}

fn patch_json(stdin: &str) -> serde_json::Value {
    serde_json::from_str(stdin).unwrap_or_else(|e| panic!("patch isn't JSON ({e}): {stdin:.40}"))
}

#[test]
fn adds_both_keys_restarts_and_drops_the_old_token_after_sign_in() {
    let r = run(KEY, "GITHUB_TOKEN\nLINEAR_API_KEY\n", 0, SIGNED_IN);
    assert_eq!(r.code, 0, "{}", r.out);
    let patches = r.patches();
    assert_eq!(patches.len(), 2, "one patch to add, one to drop GITHUB_TOKEN: {:?}", r.calls);
    let add = patch_json(&patches[0].1);
    assert_eq!(add["stringData"]["GITHUB_APP_ID"], "42");
    // The key goes in as its own text, real line breaks and all.
    assert_eq!(add["stringData"]["GITHUB_APP_PRIVATE_KEY"].as_str().unwrap().trim(), KEY.trim());
    assert!(add["stringData"]["GITHUB_APP_PRIVATE_KEY"].as_str().unwrap().contains("\n"));
    // A merge patch names only what it adds, so the other keys stay.
    assert_eq!(add.as_object().unwrap().len(), 1);
    assert_eq!(add["stringData"].as_object().unwrap().len(), 2);
    assert!(patches[0].0.contains(&"merge".to_string()));
    assert_eq!(r.restarts(), 1);
    // The old token goes last, only after the pod signed in as the App.
    let drop = &patches[1].0.join(" ");
    assert!(drop.contains("/data/GITHUB_TOKEN") && !drop.contains("GITHUB_APP"), "{drop}");
    let order: Vec<String> = r.calls.iter().map(|(a, _)| a.join(" ")).collect();
    let logs = order.iter().position(|c| c.contains("logs")).expect("the pod's logs are read");
    let dropped = order.iter().position(|c| c.contains("/data/GITHUB_TOKEN")).unwrap();
    assert!(logs < dropped, "{order:#?}");
    assert!(r.out.contains("42"), "{}", r.out);
    assert_no_leak(&r);
}

#[test]
fn leaves_the_secret_without_a_token_alone_after_sign_in() {
    let r = run(KEY, "LINEAR_API_KEY\n", 0, SIGNED_IN);
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(r.patches().len(), 1, "{:?}", r.calls);
    assert_no_leak(&r);
}

/// AC-4: both keys out, a restart, the token untouched, the step named.
fn assert_backed_out(r: &Run, step: &str) {
    assert_ne!(r.code, 0, "{}", r.out);
    let patches = r.patches();
    assert_eq!(patches.len(), 2, "{:?}", r.calls);
    let back = patches[1].0.join(" ");
    assert!(back.contains("/data/GITHUB_APP_ID") && back.contains("/data/GITHUB_APP_PRIVATE_KEY"), "{back}");
    assert!(!r.calls.iter().any(|(a, _)| a.join(" ").contains("/data/GITHUB_TOKEN")), "{:?}", r.calls);
    assert_eq!(r.restarts(), 2, "one to try the App, one to go back");
    assert!(r.out.contains(step), "the failed step isn't named:\n{}", r.out);
    assert_no_leak(r);
}

#[test]
fn backs_out_when_the_pod_does_not_come_up() {
    let r = run(KEY, "GITHUB_TOKEN\nLINEAR_API_KEY\n", 1, SIGNED_IN);
    assert_backed_out(&r, "rollout status");
}

#[test]
fn backs_out_when_the_pod_does_not_sign_in_as_the_app() {
    let r = run(KEY, "GITHUB_TOKEN\nLINEAR_API_KEY\n", 0, r#"{"fields":{"message":"listening"}}"#);
    assert_backed_out(&r, "sign-in");
}

#[test]
fn a_second_run_changes_nothing() {
    let r = run(KEY, "GITHUB_APP_ID\nGITHUB_APP_PRIVATE_KEY\nLINEAR_API_KEY\n", 0, SIGNED_IN);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.patches().is_empty() && r.restarts() == 0, "{:?}", r.calls);
    assert!(r.out.contains("already set up"), "{}", r.out);
    assert!(r.out.contains("GITHUB_APP_ID"), "says how to replace it: {}", r.out);
    assert_no_leak(&r);
}

#[test]
fn a_key_that_does_not_parse_touches_nothing() {
    let bad = KEY.replace("MII", "XXX");
    let r = run(&bad, "LINEAR_API_KEY\n", 0, SIGNED_IN);
    assert_ne!(r.code, 0, "{}", r.out);
    // Only the read of the key names: no patch, no restart.
    assert!(r.calls.iter().all(|(a, _)| a.contains(&"get".to_string())), "kubectl changed something: {:?}", r.calls);
    assert!(r.out.contains("GITHUB_APP_PRIVATE_KEY"), "{}", r.out);
    for line in bad.lines().filter(|l| !l.starts_with("-----") && l.len() > 16) {
        assert!(!r.out.contains(line), "the key is in the output:\n{}", r.out);
    }
}

#[test]
fn a_second_run_without_the_key_still_says_it_is_set_up() {
    // The README has the owner delete the .pem after the first run.
    let r = run("", "GITHUB_APP_ID\nGITHUB_APP_PRIVATE_KEY\nLINEAR_API_KEY\n", 0, SIGNED_IN);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.patches().is_empty() && r.restarts() == 0, "{:?}", r.calls);
    assert!(r.out.contains("already set up"), "{}", r.out);
}

#[test]
fn reads_the_secret_by_key_names_only() {
    let r = run(KEY, "LINEAR_API_KEY\n", 0, SIGNED_IN);
    let get = r.find(&["get", "secret", "sdlc-board-tokens"]);
    assert_eq!(get.len(), 1);
    let args = get[0].0.join(" ");
    // A template over the keys: no value ever reaches the setup's output.
    assert!(args.contains("go-template") && args.contains("range $k"), "{args}");
    // Every call is bounded: a request timeout, or the watch's own --timeout.
    let bounded = |a: &Vec<String>| a.iter().any(|x| x.starts_with("--request-timeout") || x.starts_with("--timeout"));
    assert!(r.calls.iter().all(|(a, _)| bounded(a)), "{:?}", r.calls);
    let status = r.find(&["rollout", "status"]);
    assert!(!status[0].0.iter().any(|x| x.starts_with("--request-timeout")), "it would cut the watch short");
}

#[test]
fn a_failed_patch_that_quotes_the_key_shows_it_redacted() {
    let r = run_with(KEY, "LINEAR_API_KEY\n", 0, SIGNED_IN, &[("FAKE_PATCH_FAIL", "1")]);
    assert_ne!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("add the App to the Secret") && r.out.contains("[redacted]"), "{}", r.out);
    assert_eq!(r.restarts(), 0);
    assert_no_leak(&r);
}
