//! The owner's one-time setup of the board's GitHub App on the cluster. The
//! key only ever travels through memory and `kubectl`'s stdin: never an
//! argument (other users can read command lines), a file or the output.

use anyhow::{bail, Context};
use board_server::github_app::Settings;
use std::io::Write;
use std::process::{Command, Stdio};

pub mod app;

pub const NAMESPACE: &str = "sdlc-board";
pub const SECRET: &str = "sdlc-board-tokens";
pub const DEPLOYMENT: &str = "sdlc-board";
/// What the server logs at start once it has the App's settings.
pub const SIGNED_IN: &str = "reading GitHub as the GitHub App";

/// Runs `kubectl` (or `KUBECTL`) against the owner's current context.
pub struct Kubectl {
    program: String,
    /// Removed from anything `kubectl` says before it's shown.
    key: String,
}

impl Kubectl {
    pub fn from_env(key: &str) -> Self {
        Self { program: std::env::var("KUBECTL").unwrap_or_else(|_| "kubectl".into()), key: key.into() }
    }

    /// Runs one command, `stdin` piped in; its stdout, or an error with its
    /// stderr, scrubbed of the key.
    fn run(&self, args: &[&str], stdin: &str) -> anyhow::Result<String> {
        // `rollout status` is a watch with its own --timeout; a request
        // timeout would cut it off early and back out a healthy pod.
        let watch = args.starts_with(&["rollout", "status"]);
        let mut child = Command::new(&self.program)
            .args(if watch { vec!["-n", NAMESPACE] } else { vec!["--request-timeout=30s", "-n", NAMESPACE] })
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("can't run {}", self.program))?;
        // Dropped after the write, so kubectl sees the end of its input.
        child.stdin.take().context("kubectl's stdin")?.write_all(stdin.as_bytes())?;
        let out = child.wait_with_output()?;
        let (stdout, stderr) =
            (self.scrub(&String::from_utf8_lossy(&out.stdout)), self.scrub(&String::from_utf8_lossy(&out.stderr)));
        if !out.status.success() {
            bail!("kubectl {} failed: {}", args.join(" "), stderr.trim());
        }
        Ok(stdout)
    }

    /// Any line of the key's body, should an error quote the patch.
    fn scrub(&self, text: &str) -> String {
        let mut text = text.to_string();
        for line in self.key.lines().map(str::trim).filter(|l| l.len() > 16 && !l.starts_with("-----")) {
            text = text.replace(line, "[redacted]");
        }
        text
    }

    /// The Secret's key names; a template over `.data` so no value is read.
    fn secret_keys(&self) -> anyhow::Result<Vec<String>> {
        let template = r#"go-template={{range $k, $v := .data}}{{$k}}{{"\n"}}{{end}}"#;
        let out = self.run(&["get", "secret", SECRET, "-o", template], "")?;
        Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }

    fn remove_keys(&self, keys: &[&str]) -> anyhow::Result<()> {
        let ops: Vec<String> = keys.iter().map(|k| format!(r#"{{"op":"remove","path":"/data/{k}"}}"#)).collect();
        self.run(&["patch", "secret", SECRET, "--type", "json", "-p", &format!("[{}]", ops.join(","))], "")?;
        Ok(())
    }

    fn restart(&self) -> anyhow::Result<()> {
        self.run(&["rollout", "restart", &format!("deployment/{DEPLOYMENT}")], "")?;
        Ok(())
    }
}

/// Puts the App's ID and key in the board's Secret, restarts the board and
/// checks it signed in as the App, then drops `GITHUB_TOKEN`. A failure after
/// the Secret changed takes the App back out, so the board isn't left down.
pub fn install_key(kube: &Kubectl, app_id: &str, pem: &str, out: &mut impl Write) -> anyhow::Result<()> {
    // First, so a second run needs no key and still says what to do.
    let keys = kube.secret_keys().context("step: read the Secret's keys")?;
    if already_set_up(&keys, out)? {
        return Ok(());
    }
    // The server's own check, before anything changes on the cluster.
    Settings::from_values(Some(app_id), Some(pem))?;
    let patch = serde_json::json!({ "stringData": {
        "GITHUB_APP_ID": app_id.trim(),
        "GITHUB_APP_PRIVATE_KEY": format!("{}\n", pem.trim()),
    }});
    kube.run(&["patch", "secret", SECRET, "--type", "merge", "--patch-file", "/dev/stdin"], &patch.to_string())
        .context("step: add the App to the Secret")?;
    writeln!(out, "Added GITHUB_APP_ID {} and the key to {SECRET}; restarting the board.", app_id.trim())?;
    if let Err(e) = check_sign_in(kube) {
        let back = kube.remove_keys(&["GITHUB_APP_ID", "GITHUB_APP_PRIVATE_KEY"]).and_then(|_| kube.restart());
        return Err(match back {
            Ok(()) => e.context("the App's keys were removed again and the board restarted"),
            Err(b) => e.context(format!(
                "and taking the App back out failed too ({b:#}): run argocd/README.md's \"To back out\" by hand"
            )),
        });
    }
    if keys.iter().any(|k| k == "GITHUB_TOKEN") {
        kube.remove_keys(&["GITHUB_TOKEN"]).context("step: remove GITHUB_TOKEN (the App works; remove it by hand)")?;
        writeln!(out, "Removed GITHUB_TOKEN, which the App replaces.")?;
    }
    writeln!(out, "Done: the board reads GitHub as App {}.", app_id.trim())?;
    Ok(())
}

/// True, having said so and how to replace it, when the Secret already has an App.
fn already_set_up(keys: &[String], out: &mut impl Write) -> anyhow::Result<bool> {
    if !keys.iter().any(|k| k == "GITHUB_APP_ID") {
        return Ok(false);
    }
    writeln!(
        out,
        "The GitHub App is already set up: {SECRET} has GITHUB_APP_ID. Nothing changed.\n\
         To replace it on purpose, remove GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY first \
         (argocd/README.md, \"To back out\"), then run this again."
    )?;
    Ok(true)
}

/// The `app` command's first step: true when there's nothing to do.
pub fn is_set_up(kube: &Kubectl, out: &mut impl Write) -> anyhow::Result<bool> {
    let keys = kube.secret_keys().context("step: read the Secret's keys")?;
    already_set_up(&keys, out)
}

fn check_sign_in(kube: &Kubectl) -> anyhow::Result<()> {
    kube.restart().context("step: rollout restart")?;
    kube.run(&["rollout", "status", &format!("deployment/{DEPLOYMENT}"), "--timeout=2m"], "")
        .context("step: rollout status (the new pod didn't become ready in 2 minutes)")?;
    let logs = kube.run(&["logs", &format!("deployment/{DEPLOYMENT}")], "").context("step: read the pod's logs")?;
    if !logs.contains(SIGNED_IN) {
        bail!("step: sign-in check (the new pod didn't log \"{SIGNED_IN}\")");
    }
    Ok(())
}
