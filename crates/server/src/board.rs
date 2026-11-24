use board_api::{Column, Ticket, STAGES};
use std::collections::HashMap;

/// A ticket as a source reports it, before nesting.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub url: String,
    pub repo: String,
    /// The ticket's label in the Stage group, if it has one.
    pub stage: Option<String>,
    pub parent: Option<String>,
    pub open: bool,
}

/// Places each open ticket once: a sub-ticket under its parent when the
/// parent is open too, otherwise in its own stage's column. A ticket with no
/// Stage label hasn't entered the pipeline, so it counts as Intake.
pub fn columns(issues: &[Issue]) -> Vec<Column> {
    let open: Vec<&Issue> = issues.iter().filter(|i| i.open).collect();
    let open_ids: HashMap<&str, ()> = open.iter().map(|i| (i.id.as_str(), ())).collect();
    let mut children: HashMap<&str, Vec<&Issue>> = HashMap::new();
    let mut roots = Vec::new();
    for issue in &open {
        match issue.parent.as_deref() {
            Some(p) if open_ids.contains_key(p) => children.entry(p).or_default().push(issue),
            _ => roots.push(*issue),
        }
    }
    let mut cols: Vec<Column> = STAGES.iter().map(|s| Column { stage: (*s).into(), tickets: vec![] }).collect();
    for root in roots {
        let ticket = nest(root, &children);
        let i = STAGES.iter().position(|s| *s == ticket.stage).unwrap_or(0);
        cols[i].tickets.push(ticket);
    }
    for col in &mut cols {
        col.tickets.sort_by(|a, b| a.id.cmp(&b.id));
    }
    cols
}

fn nest(issue: &Issue, children: &HashMap<&str, Vec<&Issue>>) -> Ticket {
    let stage = issue.stage.clone().filter(|s| STAGES.contains(&s.as_str())).unwrap_or_else(|| STAGES[0].into());
    let mut kids: Vec<Ticket> =
        children.get(issue.id.as_str()).map(|v| v.iter().map(|c| nest(c, children)).collect()).unwrap_or_default();
    kids.sort_by(|a, b| a.id.cmp(&b.id));
    Ticket {
        id: issue.id.clone(),
        title: issue.title.clone(),
        url: issue.url.clone(),
        repo: issue.repo.clone(),
        stage,
        children: kids,
    }
}
