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
