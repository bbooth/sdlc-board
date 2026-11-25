use board_app::{BoardApp, Settings};
use egui_kittest::{kittest::Queryable, Harness};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

fn token() -> Settings {
    Settings { url: "https://board.example".into(), client_id: "id.access".into(), client_secret: "s3cret".into() }
}

// AC-8, Android side: settings saved in the app's storage come back as they were.
#[test]
fn settings_round_trip_through_storage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(Settings::load(&path).unwrap(), None);
    token().save(&path).unwrap();
    assert_eq!(Settings::load(&path).unwrap(), Some(token()));
    let changed = Settings { url: "https://other.example".into(), ..token() };
    changed.save(&path).unwrap();
    assert_eq!(Settings::load(&path).unwrap(), Some(changed));
}

#[test]
fn a_token_is_both_headers_or_none() {
    let headers = token().headers().unwrap();
    assert_eq!(headers, [("CF-Access-Client-Id", "id.access".into()), ("CF-Access-Client-Secret", "s3cret".into())]);
    assert_eq!(Settings::default().headers().unwrap(), []);
    let half = Settings { client_secret: String::new(), ..token() };
    assert!(half.headers().is_err());
}

// AC-8: the service token goes with each read of the snapshot as Access's two headers.
#[test]
fn reads_send_the_token_as_the_access_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/snapshot", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut req = Vec::new();
        let mut buf = [0; 1024];
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = conn.read(&mut buf).unwrap();
            req.extend_from_slice(&buf[..n]);
        }
        conn.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok").unwrap();
        String::from_utf8(req).unwrap().to_ascii_lowercase()
    });
    let (tx, rx) = mpsc::channel();
    board_app::get(url, &token(), move |body| tx.send(body).unwrap());
    assert_eq!(rx.recv().unwrap().unwrap(), b"ok");
    let req = server.join().unwrap();
    assert!(req.contains("\r\ncf-access-client-id: id.access\r\n"), "{req}");
    assert!(req.contains("\r\ncf-access-client-secret: s3cret\r\n"), "{req}");
}

// AC-8, Android side: with nothing saved the app opens its settings, and Save keeps them.
#[test]
fn the_settings_form_saves_to_storage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let store = Some(path.clone());
    let mut harness = Harness::new_eframe(|_| BoardApp::new(Settings::default(), store));
    harness.step();
    // Without an address there's no board to read, so Save refuses.
    harness.get_by_label("Save").click();
    harness.step();
    harness.get_by_label("Couldn't save: set the board's address");
    assert_eq!(Settings::load(&path).unwrap(), None);
    // Pasted values often carry spaces; they are saved without them.
    let typed = [
        ("Board address", " https://board.example "),
        ("Access client ID", "id.access"),
        ("Access client secret", "s3cret"),
    ];
    for (name, text) in typed {
        let field = harness.get_by_label_contains(name);
        field.click();
        field.type_text(text);
        harness.step();
    }
    harness.get_by_label("Save").click();
    harness.step();
    assert_eq!(Settings::load(&path).unwrap(), Some(token()));
    harness.get_by_label("Settings");
}
