use board_core::Settings;

fn token() -> Settings {
    Settings { url: "https://board.example".into(), client_id: "id.access".into(), client_secret: "s3cret".into() }
}

// AC-2 of TRU-13: settings are saved trimmed and come back as they were saved.
#[test]
fn settings_are_saved_trimmed_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(Settings::load(&path).unwrap(), None);
    let pasted = Settings { url: " https://board.example \n".into(), client_secret: " s3cret".into(), ..token() };
    assert_eq!(pasted.save(&path).unwrap(), token());
    assert_eq!(Settings::load(&path).unwrap(), Some(token()));
}

#[test]
fn settings_that_cant_read_a_board_are_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let no_address = Settings { url: "  ".into(), ..token() };
    assert_eq!(no_address.save(&path).unwrap_err(), "set the board's address");
    let half = Settings { client_id: String::new(), ..token() };
    assert_eq!(half.save(&path).unwrap_err(), "set both the Access client ID and secret, or neither");
    assert_eq!(Settings::load(&path).unwrap(), None);
}

#[test]
fn a_token_is_both_headers_or_none() {
    let headers = token().headers().unwrap();
    assert_eq!(headers, [("CF-Access-Client-Id", "id.access".into()), ("CF-Access-Client-Secret", "s3cret".into())]);
    assert_eq!(Settings { url: "u".into(), ..Default::default() }.headers().unwrap(), []);
}
