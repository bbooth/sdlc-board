use board_api::Snapshot;
use egui_kittest::{kittest::Queryable, Harness};

const SNAPSHOT: &str = r#"{"generated_at": 0, "sources": [], "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build",
  "tickets": [{"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
    {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "Verify", "children": [
      {"id": "TRU-10", "title": "b", "url": "u", "repo": "r", "stage": "Plan", "children": []}]}]}]}]}"#;

// AC-1, app side: each stage gets a column and a ticket's sub-tickets are
// drawn under it with their own stages.
#[test]
fn sub_tickets_are_drawn_under_their_ticket_with_their_stages() {
    let snap: Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    let harness = Harness::new_ui(|ui| board_app::board(ui, &snap));
    for text in ["Spec (0)", "Build (1)", "TRU-8", "One board", "TRU-9", "· Verify", "TRU-10", "· Plan"] {
        harness.get_by_label(text);
    }
}
