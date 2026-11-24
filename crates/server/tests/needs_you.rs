use board_api::{Checks, Pr};
use board_server::{board, board::Issue, github::PrInfo};

fn issue(id: &str, stage: &str, open: bool) -> Issue {
    let url = format!("https://linear.app/t/{id}");
    Issue {
        id: id.into(),
        title: format!("{id} title"),
        url,
        repo: "r".into(),
        stage: Some(stage.into()),
        parent: None,
        open,
        gates: vec![],
    }
}

fn pr(ticket: &str, number: u64, checks: Checks, draft: bool) -> PrInfo {
    let url = format!("https://github.com/bbooth/booking/pull/{number}");
    let pr = Pr { number, title: format!("{ticket}: pr"), url, checks };
    PrInfo { ticket: ticket.into(), pr, draft, by_branch: true, gate: None }
}

#[test]
fn specs_to_approve_green_prs_and_blocked_tickets_need_the_owner() {
    let issues = [
        issue("TRU-8", "Ready for approval", true),
        issue("TRU-11", "Blocked", true),
        issue("TRU-12", "Build", true),
        issue("TRU-6", "PR review", true),
        issue("TRU-7", "PR review", true),
        issue("TRU-9", "PR review", true),
        issue("TRU-10", "PR review", true),
        issue("TRU-3", "Blocked", false),
    ];
    let prs = [
        pr("TRU-6", 1, Checks::Pass, false),
        pr("TRU-7", 2, Checks::Pending, false),
        pr("TRU-9", 3, Checks::Fail, false),
        pr("TRU-10", 4, Checks::Pass, true),
        // Green, but its review hasn't run yet.
        pr("TRU-12", 5, Checks::Pass, false),
    ];
    let items = board::needs_you(&issues, &prs);

    let got: Vec<(&str, &str, &str)> = items.iter().map(|n| (n.kind.as_str(), n.id.as_str(), n.url.as_str())).collect();
    assert_eq!(
        got,
        [
            ("approve", "TRU-8", "https://linear.app/t/TRU-8"),
            ("merge", "TRU-6", "https://github.com/bbooth/booking/pull/1"),
            ("blocked", "TRU-11", "https://linear.app/t/TRU-11"),
        ]
    );
}
