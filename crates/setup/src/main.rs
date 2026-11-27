//! `board-setup` (or `board-setup app`) creates the board's GitHub App and
//! puts it on the cluster; `board-setup key --app-id ID < app.pem` does the
//! cluster half for an App made by hand (argocd/README.md step 4).

use std::io::Read;

const USAGE: &str = "usage: board-setup [app] | board-setup key --app-id ID < app.pem";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app_id = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["app"] => {
            if let Err(e) = board_setup::app::run(&board_setup::app::Env::from_env(), &mut std::io::stdout()) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
            return;
        }
        ["key", "--app-id", id] => id.to_string(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let mut pem = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut pem) {
        eprintln!("can't read the key on stdin: {e}");
        std::process::exit(1);
    }
    let kube = board_setup::Kubectl::from_env(&pem);
    if let Err(e) = board_setup::install_key(&kube, &app_id, &pem, &mut std::io::stdout()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
