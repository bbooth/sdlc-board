//! Creates the board's GitHub App through GitHub's manifest flow, so the
//! owner confirms one page instead of filling in a form, then installs it on
//! exactly the board's repos. The App's key arrives in GitHub's answer and
//! stays in memory until `install_key` pipes it to kubectl.

use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long the owner has to get through GitHub's two pages.
const BROWSER_WAIT: Duration = Duration::from_secs(15 * 60);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Where GitHub and the owner's tools are; the environment overrides each,
/// so tests run against a stub.
pub struct Env {
    pub web: String,
    pub api: String,
    pub gh: String,
    pub open: String,
    pub values: String,
}

impl Env {
    pub fn from_env() -> Self {
        let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Self {
            web: var("GITHUB_WEB", "https://github.com").trim_end_matches('/').into(),
            api: var("GITHUB_API", "https://api.github.com").trim_end_matches('/').into(),
            gh: var("GH", "gh"),
            open: var("BOARD_SETUP_OPEN", "xdg-open"),
            values: var("BOARD_VALUES", concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/values.yaml")),
        }
    }
}

/// The App GitHub creates: read-only on what the board reads, no webhook,
/// installable only on the owner's account.
pub fn manifest(name: &str, redirect: &str, setup: &str) -> Value {
    json!({
        "name": name,
        "url": "https://board.trulinc.com",
        "description": "Read-only: the sdlc-board reads PRs, checks and gate records.",
        "public": false,
        "hook_attributes": { "url": "https://board.trulinc.com", "active": false },
        "redirect_url": redirect,
        "setup_url": setup,
        "setup_on_update": false,
        "default_permissions": {
            "checks": "read", "contents": "read", "metadata": "read", "pull_requests": "read",
        },
        "default_events": [],
    })
}

/// The repos in `values.yaml`'s top-level `repos` list, by `name`.
pub fn board_repos(values: &str) -> Vec<String> {
    let mut repos = vec![];
    let mut inside = false;
    for line in values.lines() {
        if !line.starts_with(' ') && !line.starts_with('#') && !line.trim().is_empty() {
            inside = line.trim_end() == "repos:";
            continue;
        }
        if let Some(name) = inside.then(|| line.trim().strip_prefix("- name:")).flatten() {
            repos.push(name.trim().trim_matches('"').to_string());
        }
    }
    repos
}

pub struct CreatedApp {
    pub id: u64,
    pub slug: String,
    pub pem: String,
}

/// Runs the flow, then `install_key`.
pub fn run(env: &Env, out: &mut impl Write) -> anyhow::Result<()> {
    if crate::is_set_up(&crate::Kubectl::from_env(""), out)? {
        return Ok(());
    }
    let values = std::fs::read_to_string(&env.values).with_context(|| format!("can't read {}", env.values))?;
    let repos = board_repos(&values);
    if repos.is_empty() {
        bail!("{} lists no repos", env.values);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CALL_TIMEOUT))
        .http_status_as_error(false)
        .user_agent("board-setup")
        .build()
        .into();
    let (app, installation) = create_and_install(env, &agent, out)?;
    writeln!(out, "Created the GitHub App {} (ID {}), installed as {installation}.", app.slug, app.id)?;
    match match_repos(env, &agent, installation, &repos, out) {
        Ok(()) => writeln!(out, "The App is installed on exactly: {}.", repos.join(", "))?,
        // The board still works for the repos that are on it; say what's left.
        Err(e) => writeln!(
            out,
            "Couldn't set the App's repos ({e:#}). Choose \"Only select repositories\" and exactly {} at {}/settings/installations/{installation}.",
            repos.join(", "),
            env.web
        )?,
    }
    crate::install_key(&crate::Kubectl::from_env(&app.pem), &app.id.to_string(), &app.pem, out)
}

/// Serves the manifest page on a local port, turns GitHub's code into the
/// App, sends the browser on to the install page and waits for its redirect.
fn create_and_install(env: &Env, agent: &ureq::Agent, out: &mut impl Write) -> anyhow::Result<(CreatedApp, u64)> {
    let listener = TcpListener::bind("127.0.0.1:0").context("can't listen on 127.0.0.1")?;
    let base = format!("http://{}", listener.local_addr()?);
    let state = random_hex()?;
    let name = format!("sdlc-board-{}", &state[..6]);
    let manifest = manifest(&name, &format!("{base}/created"), &format!("{base}/installed"));
    writeln!(out, "Opening {base}/ in your browser: confirm creating the App, then install it.")?;
    // The URL holds nothing secret; a missing opener only means a manual click.
    let _ = Command::new(&env.open).arg(format!("{base}/")).spawn();
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + BROWSER_WAIT;
    let mut app: Option<CreatedApp> = None;
    loop {
        if Instant::now() > deadline {
            bail!("step: GitHub's pages (nothing came back in {} minutes)", BROWSER_WAIT.as_secs() / 60);
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(CALL_TIMEOUT))?;
        let (path, query) = match read_request(&mut stream) {
            Ok(r) => r,
            Err(_) => continue,
        };
        match (path.as_str(), &app) {
            ("/", None) => {
                let action = format!("{}/settings/apps/new?state={state}", env.web);
                respond(&mut stream, 200, &form_page(&action, &manifest.to_string()))?;
            }
            ("/created", None) => {
                if param(&query, "state").as_deref() != Some(state.as_str()) {
                    respond(&mut stream, 400, "This doesn't match the setup that's running.")?;
                    continue;
                }
                let code = param(&query, "code").context("GitHub's redirect has no code")?;
                let created = convert(env, agent, &code).context("step: create the App from GitHub's code")?;
                redirect(&mut stream, &format!("{}/apps/{}/installations/new", env.web, created.slug))?;
                app = Some(created);
            }
            ("/installed", Some(_)) => {
                let Some(id) = param(&query, "installation_id").and_then(|i| i.parse().ok()) else {
                    respond(&mut stream, 400, "GitHub's redirect has no installation_id.")?;
                    continue;
                };
                respond(&mut stream, 200, "Installed. You can close this tab and go back to the terminal.")?;
                return Ok((app.take().expect("matched Some"), id));
            }
            _ => respond(&mut stream, 404, "Not part of the setup.")?,
        }
    }
}

/// `POST /app-manifests/{code}/conversions`: the App's ID, slug and key.
fn convert(env: &Env, agent: &ureq::Agent, code: &str) -> anyhow::Result<CreatedApp> {
    if !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("GitHub's code isn't alphanumeric");
    }
    let mut resp = agent
        .post(format!("{}/app-manifests/{code}/conversions", env.api))
        .header("Accept", "application/vnd.github+json")
        .send_empty()?;
    if !resp.status().is_success() {
        bail!("GitHub answered {}", resp.status());
    }
    let body: Value = serde_json::from_slice(&resp.body_mut().read_to_vec()?)?;
    let id = body["id"].as_u64().context("no App id in GitHub's answer")?;
    let slug = body["slug"].as_str().context("no App slug in GitHub's answer")?.to_string();
    let pem = body["pem"].as_str().context("no key in GitHub's answer")?.to_string();
    Ok(CreatedApp { id, slug, pem })
}

/// Adds the board's repos the installation lacks and removes the rest,
/// with the owner's own `gh` sign-in (the App can't change its own repos).
fn match_repos(
    env: &Env,
    agent: &ureq::Agent,
    installation: u64,
    want: &[String],
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let token = gh_token(env)?;
    let call = |method: &str, path: &str| -> anyhow::Result<Value> {
        let url = format!("{}{path}", env.api);
        let req = ureq::http::Request::builder()
            .method(method)
            .uri(&url)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {token}"))
            .body(())?;
        let mut resp = agent.run(req)?;
        if !resp.status().is_success() {
            bail!("{method} {path}: GitHub answered {}", resp.status());
        }
        let body = resp.body_mut().read_to_vec()?;
        Ok(if body.is_empty() { Value::Null } else { serde_json::from_slice(&body)? })
    };
    let mut have: Vec<(u64, String)> = vec![];
    for page in 1.. {
        let body = call("GET", &format!("/user/installations/{installation}/repositories?per_page=100&page={page}"))?;
        let repos = body["repositories"].as_array().cloned().unwrap_or_default();
        have.extend(repos.iter().filter_map(|r| Some((r["id"].as_u64()?, r["full_name"].as_str()?.to_string()))));
        if repos.len() < 100 {
            break;
        }
    }
    for name in want.iter().filter(|w| !have.iter().any(|(_, h)| h.eq_ignore_ascii_case(w))) {
        let id = call("GET", &format!("/repos/{name}"))?["id"].as_u64().with_context(|| format!("no id for {name}"))?;
        call("PUT", &format!("/user/installations/{installation}/repositories/{id}"))?;
        writeln!(out, "Added {name} to the App.")?;
    }
    for (id, name) in have.iter().filter(|(_, h)| !want.iter().any(|w| w.eq_ignore_ascii_case(h))) {
        call("DELETE", &format!("/user/installations/{installation}/repositories/{id}"))?;
        writeln!(out, "Removed {name} from the App: it isn't on the board.")?;
    }
    Ok(())
}

fn gh_token(env: &Env) -> anyhow::Result<String> {
    let out =
        Command::new(&env.gh).args(["auth", "token"]).output().with_context(|| format!("can't run {}", env.gh))?;
    let token = String::from_utf8(out.stdout)?.trim().to_string();
    if !out.status.success() || token.is_empty() {
        bail!("`gh auth token` gave no token: sign in with `gh auth login`");
    }
    Ok(token)
}

fn random_hex() -> anyhow::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The request line's path and query; the rest of the request is ignored.
fn read_request(stream: &mut TcpStream) -> anyhow::Result<(String, String)> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") && len < buf.len() {
        let n = stream.read(&mut buf[len..])?;
        if n == 0 {
            break;
        }
        len += n;
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let target = head.split_whitespace().nth(1).context("no request line")?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Ok((path.to_string(), query.to_string()))
}

fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v.to_string()))
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) -> anyhow::Result<()> {
    let page = format!("<!doctype html><meta charset=utf-8><title>board-setup</title><body>{body}</body>");
    write!(
        stream,
        "HTTP/1.1 {status} OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    )?;
    Ok(())
}

fn redirect(stream: &mut TcpStream, to: &str) -> anyhow::Result<()> {
    write!(stream, "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    Ok(())
}

/// A form that posts the manifest to GitHub as soon as it loads.
fn form_page(action: &str, manifest: &str) -> String {
    format!(
        r#"<form id="f" method="post" action="{}"><input type="hidden" name="manifest" value="{}">
<p>Sending the App's settings to GitHub... <button>Continue</button></p></form>
<script>document.getElementById("f").submit()</script>"#,
        escape(action),
        escape(manifest)
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}
