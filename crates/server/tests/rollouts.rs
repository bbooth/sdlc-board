use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use board_api::{Checks, Pr, Rollout, Snapshot, Ticket};
use board_server::{board, board::Issue, cluster, cluster::Cluster, config::Repo, github::PrInfo, sources};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

const LIST: &str = include_str!("fixtures/rollouts.json");

fn issue(id: &str, repo: &str, stage: &str) -> Issue {
    let (title, url) = (format!("{id} title"), format!("https://linear.app/t/{id}"));
    let (repo, stage) = (repo.into(), Some(stage.into()));
    Issue { id: id.into(), title, url, repo, stage, parent: None, open: true, gates: vec![] }
}

/// Stands in for Linear: the tickets are all the snapshot needs from it.
struct Tickets(Vec<Issue>);

impl sources::Source for Tickets {
    fn name(&self) -> &'static str {
        "linear"
    }
    async fn read(&self) -> anyhow::Result<sources::Read> {
        Ok(sources::Read { issues: self.0.clone(), ..sources::Read::default() })
    }
}

fn repos() -> Vec<Repo> {
    let repo = |name: &str, ns: Option<&str>| Repo {
        name: name.into(),
        linear_team: "TRU".into(),
        linear_project: "p".into(),
        rollout_namespace: ns.map(Into::into),
    };
    vec![repo("bbooth/booking", Some("booking")), repo("bbooth/sdlc-board", None)]
}

/// The cluster's API, listing the recorded Rollouts in `booking`, or refusing while `forbidden`.
async fn api(forbidden: Arc<AtomicBool>) -> String {
    let list = move || async move {
        if forbidden.load(Relaxed) {
            return (StatusCode::FORBIDDEN, "rollouts.argoproj.io is forbidden").into_response();
        }
        Json(serde_json::from_str::<Value>(LIST).unwrap()).into_response()
    };
    let app = Router::new().route("/apis/argoproj.io/v1alpha1/namespaces/booking/rollouts", get(list));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

fn card<'a>(snap: &'a Snapshot, id: &str) -> &'a Ticket {
    snap.columns.iter().flat_map(|c| &c.tickets).find(|t| t.id == id).unwrap_or_else(|| panic!("no card {id}"))
}

fn web() -> Rollout {
    let (name, phase, analysis) = ("web".into(), "Paused".into(), Some("Running".into()));
    Rollout { name, step: 2, steps: 5, weight: 20, phase, analysis }
}

// AC-4: of booking's two Rollouts, only web is in flight (identity is
// healthy and complete), and it shows on the card of the booking ticket
// being rolled out, with its step, weight and analysis. Other cards,
// including another repo's ticket in Rollout, show none.
#[tokio::test]
async fn only_a_rollout_in_flight_shows_on_its_tickets_card() {
    let url = api(Arc::default()).await;
    let state = sources::State::new();
    let tickets = vec![
        issue("TRU-6", "bbooth/booking", "Rollout"),
        issue("TRU-7", "bbooth/booking", "Build"),
        issue("TRU-10", "bbooth/sdlc-board", "Rollout"),
    ];
    sources::refresh(&state, &Tickets(tickets)).await;
    sources::refresh(&state, &Cluster::new(&url, None, None, &repos()).unwrap()).await;
    let snap = state.snapshot();

    assert_eq!(card(&snap, "TRU-6").rollouts, [web()]);
    assert_eq!(card(&snap, "TRU-7").rollouts, []);
    assert_eq!(card(&snap, "TRU-10").rollouts, []);
    let source = snap.sources.iter().find(|s| s.name == "cluster").unwrap();
    assert!(source.last_good.is_some() && source.last_error.is_none(), "{source:?}");
}

// A ticket whose PR merged but whose Stage label hasn't moved on from PR
// review yet is being rolled out too; one whose PR is still open isn't.
#[test]
fn a_merged_ticket_still_in_pr_review_shows_its_rollout() {
    let pr = |ticket: &str, number: u64, merged: bool| {
        let url = format!("https://github.com/bbooth/booking/pull/{number}");
        let pr = Pr { number, title: ticket.into(), url, checks: Checks::Pass, merged };
        let (repo, ticket, at) = ("bbooth/booking".into(), ticket.into(), format!("sha{number}"));
        PrInfo { repo, ticket, pr, draft: false, by_branch: true, gate: None, at, gate_read: true, read_at: 0 }
    };
    let issues = [issue("TRU-6", "bbooth/booking", "PR review"), issue("TRU-7", "bbooth/booking", "PR review")];
    let prs = [pr("TRU-6", 1, true), pr("TRU-7", 2, false)];
    let rollouts = [cluster::RolloutInfo { repo: "bbooth/booking".into(), rollout: web() }];
    let cols = board::columns(&issues, &prs, &rollouts);
    let snap = Snapshot { columns: cols, ..Snapshot::default() };
    assert_eq!(card(&snap, "TRU-6").rollouts, [web()]);
    assert_eq!(card(&snap, "TRU-7").rollouts, []);
}

// Without traffic routing Argo reports no weights, so the weight is the
// last setWeight the canary has reached.
#[test]
fn without_traffic_routing_the_weight_comes_from_the_steps() {
    let obj = json!({"metadata": {"name": "scheduling"},
        "spec": {"strategy": {"canary": {"steps": [{"setWeight": 20}, {"pause": {}}, {"setWeight": 60}, {"pause": {}}]}}},
        "status": {"phase": "Progressing", "currentPodHash": "b", "stableRS": "a", "currentStepIndex": 3}});
    let got = cluster::in_flight(&obj).unwrap();
    assert_eq!((got.step, got.steps, got.weight, got.analysis), (4, 4, 60, None));
    // An aborted canary has sent all traffic back to stable.
    let aborted = json!({"status": {"abort": true, "phase": "Degraded", "currentStepIndex": 3}});
    let mut obj = obj;
    obj["status"].as_object_mut().unwrap().extend(aborted["status"].as_object().unwrap().clone());
    let got = cluster::in_flight(&obj).unwrap();
    assert_eq!((got.weight, got.phase.as_str()), (0, "Aborted"));
}

// AC-7 for the cluster: a refused read keeps the rollouts last read and
// marks the source stale with the error.
#[tokio::test]
async fn a_refused_read_keeps_the_last_rollouts_and_marks_the_cluster_stale() {
    let forbidden = Arc::new(AtomicBool::new(false));
    let url = api(forbidden.clone()).await;
    let state = sources::State::new();
    sources::refresh(&state, &Tickets(vec![issue("TRU-6", "bbooth/booking", "Rollout")])).await;
    let cluster = Cluster::new(&url, None, None, &repos()).unwrap();
    sources::refresh(&state, &cluster).await;
    forbidden.store(true, Relaxed);
    sources::refresh(&state, &cluster).await;
    let snap = state.snapshot();

    assert_eq!(card(&snap, "TRU-6").rollouts, [web()]);
    let source = snap.sources.iter().find(|s| s.name == "cluster").unwrap();
    let error = source.last_error.as_deref().unwrap_or_default();
    assert!(error.contains("booking") && error.contains("403"), "{error}");
}
