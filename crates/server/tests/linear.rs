use axum::{http::HeaderMap, http::StatusCode, routing::post, Router};
use board_server::{board, config::Repo, linear::Linear};

const FIXTURE: &str = include_str!("fixtures/linear_board.json");

// Serves the recorded response to a request carrying the key, as Linear would.
async fn fake_linear() -> String {
    let app = Router::new().route(
        "/graphql",
        post(|h: HeaderMap| async move {
            match h.get("authorization").and_then(|v| v.to_str().ok()) {
                Some("test-key") => (StatusCode::OK, FIXTURE),
                _ => (StatusCode::UNAUTHORIZED, ""),
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/graphql")
}

fn repo() -> Repo {
    Repo { name: "bbooth/booking".into(), linear_team: "TRU".into(), linear_project: "Booking".into() }
}

#[tokio::test]
async fn open_tickets_land_once_in_their_stage_with_sub_tickets_nested() {
    let endpoint = fake_linear().await;
    let issues = Linear::new(&endpoint, "test-key", vec![repo()]).unwrap().issues().await.unwrap();
    let cols = board::columns(&issues, &[]);

    let stages: Vec<&str> = cols.iter().map(|c| c.stage.as_str()).collect();
    assert_eq!(stages, board_api::STAGES);
    let ids = |stage: &str| -> Vec<String> {
        cols.iter().find(|c| c.stage == stage).unwrap().tickets.iter().map(|t| t.id.clone()).collect()
    };
    assert_eq!(ids("Outcome"), ["TRU-5"]);
    assert_eq!(ids("Ready for approval"), ["TRU-8"]);
    assert_eq!(ids("Intake"), ["TRU-4"]);
    // Sub-tickets sit under their parent, each with its own stage, not in a column.
    assert!(ids("Rollout").is_empty() && ids("Build").is_empty());
    let parent = &cols.iter().find(|c| c.stage == "Outcome").unwrap().tickets[0];
    let kids: Vec<(&str, &str)> = parent.children.iter().map(|c| (c.id.as_str(), c.stage.as_str())).collect();
    assert_eq!(kids, [("TRU-6", "Rollout"), ("TRU-7", "Build")]);
    // Gate output posted as comments gives the card its latest gate.
    let gate = parent.children[1].gate.as_ref().unwrap();
    assert_eq!((gate.stage.as_str(), gate.round, gate.pass), ("Build", 2, true));
    assert_eq!(parent.repo, "bbooth/booking");

    // Each open ticket appears exactly once; the completed one not at all.
    let mut all: Vec<String> = cols
        .iter()
        .flat_map(|c| &c.tickets)
        .flat_map(|t| std::iter::once(t.id.clone()).chain(t.children.iter().map(|c| c.id.clone())))
        .collect();
    all.sort();
    assert_eq!(all, ["TRU-4", "TRU-5", "TRU-6", "TRU-7", "TRU-8"]);
}

#[tokio::test]
async fn a_rejected_key_is_an_error_naming_the_project() {
    let endpoint = fake_linear().await;
    let err = Linear::new(&endpoint, "wrong", vec![repo()]).unwrap().issues().await.unwrap_err();
    assert!(err.to_string().contains("Booking"), "{err}");
}

#[test]
fn a_graphql_error_is_reported() {
    let err = board_server::linear::parse_page(r#"{"errors":[{"message":"bad filter"}]}"#, "r").err().unwrap();
    assert!(err.to_string().contains("bad filter"));
}
