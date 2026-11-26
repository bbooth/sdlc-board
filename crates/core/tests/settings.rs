use board_core::Settings;

fn token() -> Settings {
    Settings { url: "https://board.example".into(), client_id: "id.access".into(), client_secret: "s3cret".into() }
}

// Pasted values often carry spaces; the settings are used without them.
#[test]
fn settings_are_trimmed() {
    let pasted = Settings { url: " https://board.example \n".into(), client_secret: " s3cret".into(), ..token() };
    assert_eq!(pasted.trimmed(), token());
}

#[test]
fn settings_that_cant_read_a_board_say_why() {
    assert_eq!(Settings { url: "  ".into(), ..token() }.check().unwrap_err(), "set the board's address");
    let half = Settings { client_id: String::new(), ..token() };
    assert_eq!(half.check().unwrap_err(), "set both the Access client ID and secret, or neither");
    assert_eq!(token().check(), Ok(()));
}

#[test]
fn a_token_is_both_headers_or_none() {
    let headers = token().headers().unwrap();
    assert_eq!(headers, [("CF-Access-Client-Id", "id.access".into()), ("CF-Access-Client-Secret", "s3cret".into())]);
    assert_eq!(Settings { url: "u".into(), ..Default::default() }.headers().unwrap(), []);
}

// A settings value in a log or panic message mustn't carry the Access secret.
#[test]
fn debug_output_hides_the_secret() {
    let shown = format!("{:?}", token());
    assert!(!shown.contains("s3cret"), "{shown}");
    assert!(shown.contains("id.access") && shown.contains("<redacted>"), "{shown}");
}

// A token is only sent over https, or to this device itself.
#[test]
fn a_token_needs_an_https_address() {
    let http = |url: &str| Settings { url: url.into(), ..token() }.check();
    let refused = "use an https:// address, so the Access token isn't sent in the clear";
    assert_eq!(http("http://board.example").unwrap_err(), refused);
    assert_eq!(http("http://localhost.example:8080").unwrap_err(), refused);
    assert_eq!(http("http://localhost:x@board.example/").unwrap_err(), refused);
    assert_eq!(http("http://127.0.0.1:8080"), Ok(()));
    assert_eq!(http("http://localhost/"), Ok(()));
    assert_eq!(Settings { url: "http://board.example".into(), ..Default::default() }.check(), Ok(()));
}

#[test]
fn settings_are_saved_trimmed_for_this_user_only_and_load_back() {
    let dir = std::env::temp_dir().join(format!("board-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    assert_eq!(Settings::load(&path), None);
    Settings { client_secret: " s3cret\n".into(), ..token() }.save(&path).unwrap();
    assert_eq!(Settings::load(&path), Some(token()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // Settings that can't read a board aren't saved over the good ones.
    let half = Settings { client_id: String::new(), ..token() };
    assert_eq!(half.save(&path).unwrap_err(), "set both the Access client ID and secret, or neither");
    assert_eq!(Settings::load(&path), Some(token()));
    std::fs::remove_dir_all(dir).unwrap();
}
