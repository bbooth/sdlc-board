use crate::{cluster::RolloutInfo, gates, github::PrInfo};
use board_api::{Checks, Column, Gate, NeedsYou, Rollout, Ticket, STAGES};
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
    /// Gate results posted as comments on the ticket.
    pub gates: Vec<Gate>,
}

/// Places each open ticket once: a sub-ticket under its parent when the
/// parent is open too, otherwise in its own stage's column. A ticket with no
/// Stage label hasn't entered the pipeline, so it counts as Intake.
pub fn columns(issues: &[Issue], prs: &[PrInfo], rollouts: &[RolloutInfo]) -> Vec<Column> {
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
        let ticket = nest(root, &children, prs, rollouts);
        let i = STAGES.iter().position(|s| *s == ticket.stage).unwrap_or(0);
        cols[i].tickets.push(ticket);
    }
    for col in &mut cols {
        col.tickets.sort_by(|a, b| a.id.cmp(&b.id));
    }
    cols
}

fn nest(issue: &Issue, children: &HashMap<&str, Vec<&Issue>>, prs: &[PrInfo], rollouts: &[RolloutInfo]) -> Ticket {
    let stage = issue.stage.clone().filter(|s| STAGES.contains(&s.as_str())).unwrap_or_else(|| STAGES[0].into());
    let mut kids: Vec<Ticket> = children
        .get(issue.id.as_str())
        .map(|v| v.iter().map(|c| nest(c, children, prs, rollouts)).collect())
        .unwrap_or_default();
    kids.sort_by(|a, b| a.id.cmp(&b.id));
    let pr = pr_for(prs, issue);
    Ticket {
        id: issue.id.clone(),
        title: issue.title.clone(),
        url: issue.url.clone(),
        repo: issue.repo.clone(),
        stage,
        children: kids,
        gate: gates::latest(issue.gates.iter().chain(pr.and_then(|p| p.gate.as_ref()))),
        pr: pr.map(|p| p.pr.clone()),
        rollouts: rollouts_for(issue, pr, rollouts),
    }
}

/// A repo's rollouts in flight release its tickets being rolled out: those
/// in the Rollout stage, and those merged but not yet moved on from PR
/// review. A rollout can't say which merge it carries (images are tagged
/// `latest`), so two tickets merged close together both show it.
fn rollouts_for(issue: &Issue, pr: Option<&PrInfo>, rollouts: &[RolloutInfo]) -> Vec<Rollout> {
    let merged = pr.is_some_and(|p| p.pr.merged);
    let releasing = match issue.stage.as_deref() {
        Some("Rollout") => true,
        Some("PR review") => merged,
        _ => false,
    };
    let own = rollouts.iter().filter(|r| releasing && r.repo == issue.repo);
    own.map(|r| r.rollout.clone()).collect()
}

/// The ticket's PR, from its own repo: one whose branch names it, then an
/// open one, then the newest. Repos can share a Linear team, so another
/// repo's PR naming the ticket isn't its.
fn pr_for<'a>(prs: &'a [PrInfo], issue: &Issue) -> Option<&'a PrInfo> {
    let own = prs.iter().filter(|p| p.ticket == issue.id && p.repo == issue.repo);
    own.max_by_key(|p| (p.by_branch, !p.pr.merged, p.pr.number))
}

/// What waits on the owner: specs to approve, PRs in PR review whose checks
/// all passed, and blocked tickets.
pub fn needs_you(issues: &[Issue], prs: &[PrInfo]) -> Vec<NeedsYou> {
    let item = |kind: &str, id: &str, title: &str, url: &str| NeedsYou {
        kind: kind.into(),
        id: id.into(),
        title: title.into(),
        url: url.into(),
    };
    let in_stage = |stage: &'static str| issues.iter().filter(move |i| i.open && i.stage.as_deref() == Some(stage));
    let mut out: Vec<NeedsYou> =
        in_stage("Ready for approval").map(|i| item("approve", &i.id, &i.title, &i.url)).collect();
    let in_review = in_stage("PR review").filter_map(|i| pr_for(prs, i));
    let green = in_review.filter(|p| p.pr.checks == Checks::Pass && !p.draft && !p.pr.merged);
    out.extend(green.map(|p| item("merge", &p.ticket, &p.pr.title, &p.pr.url)));
    out.extend(in_stage("Blocked").map(|i| item("blocked", &i.id, &i.title, &i.url)));
    out
}
