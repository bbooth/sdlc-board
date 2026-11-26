use std::time::{Duration, Instant};

const SNAPSHOT: &str = r#"{"generated_at": 0, "sources": [], "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build",
  "tickets": [{"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "Verify", "children": [
      {"id": "TRU-10", "title": "b", "url": "u", "repo": "r", "stage": "Plan", "children": []}]}]}]}]}"#;

// AC-7: a failed read of the snapshot keeps the board it had.
#[test]
fn a_failed_read_keeps_the_board_it_had() {
    let (mut reads, t) = (board_core::Reads::default(), Instant::now());
    let n = reads.start(t, false).unwrap();
    reads.finish(n, Ok(serde_json::from_str(SNAPSHOT).unwrap()));
    let n = reads.start(t + secs(30), false).unwrap();
    reads.finish(n, Err("connection refused".into()));
    assert_eq!(reads.snap.as_ref().map(|s| s.columns.len()), Some(2));
    assert_eq!(reads.problem(t + secs(31)).as_deref(), Some("connection refused"));
}

// Ages are told by the server's clock, so a device clock that's off doesn't
// make stale data look fresh: the snapshot's time plus the time since it came back.
#[test]
fn ages_go_by_the_servers_clock() {
    let mut reads = board_core::Reads::default();
    assert_eq!(reads.server_now(Instant::now()), None);
    let n = reads.start(Instant::now(), false).unwrap();
    let mut snap: board_api::Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    snap.generated_at = 5000;
    reads.finish(n, Ok(snap.clone()));
    assert_eq!(reads.server_now(Instant::now() + secs(90)), Some(5090));
    // A later board resets the reference: its own time, counted from when it came back.
    let n = reads.start(Instant::now(), true).unwrap();
    snap.generated_at = 9000;
    reads.finish(n, Ok(snap));
    assert_eq!(reads.server_now(Instant::now() + secs(5)), Some(9005));
    // A failed read keeps the board's reference.
    let n = reads.start(Instant::now(), true).unwrap();
    reads.finish(n, Err("refused".into()));
    assert_eq!(reads.server_now(Instant::now() + secs(5)), Some(9005));
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

// AC-6: the app reads the snapshot every 30 seconds, one at a
// time; a read the server never answers is given up on after 20, so the
// board can't freeze on it, and its late answer can't replace a newer board.
#[test]
fn the_board_refreshes_itself_every_30_seconds() {
    let (mut reads, t) = (board_core::Reads::default(), Instant::now());
    let first = reads.start(t, false).unwrap();
    assert!(reads.waiting(t + secs(10)));
    assert_eq!(reads.start(t + secs(10), true), None, "Refresh waits for the answer");
    reads.finish(first, Err("refused".into()));
    assert_eq!(reads.start(t + secs(20), false), None);
    let asked = reads.start(t + secs(21), true).expect("Refresh reads now once answered");
    reads.finish(asked, Err("refused".into()));
    let hung = reads.start(t + secs(51), false).unwrap();
    assert_eq!(reads.problem(t + secs(76)).as_deref(), Some("no answer for 25 s"));
    assert!(!reads.waiting(t + secs(76)));
    let next = reads.start(t + secs(81), false).unwrap();
    reads.finish(next, Ok(serde_json::from_str(SNAPSHOT).unwrap()));
    reads.finish(hung, Err("late".into()));
    assert_eq!((reads.snap.is_some(), reads.problem(t + secs(82))), (true, None));
    // A given-up read's answer still counts when nothing newer has answered.
    let slow = reads.start(t + secs(111), false).unwrap();
    let _hangs = reads.start(t + secs(141), false).unwrap();
    reads.finish(slow, Err("slow but real".into()));
    assert_eq!(reads.problem(t + secs(142)).as_deref(), Some("slow but real"));
}
