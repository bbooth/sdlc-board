use board_core::{read, read_within, ReadError, Settings};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::Duration;

const BOARD: &str = r#"{"generated_at": 7, "sources": [], "columns": []}"#;

/// Answers one request with `answer` (None: never answers) and returns the request it got.
fn serve(answer: Option<&'static str>) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut req = Vec::new();
        let mut buf = [0; 1024];
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = conn.read(&mut buf).unwrap();
            req.extend_from_slice(&buf[..n]);
        }
        match answer {
            Some(a) => conn.write_all(a.as_bytes()).unwrap(),
            None => std::thread::sleep(Duration::from_secs(3)),
        }
        String::from_utf8(req).unwrap().to_ascii_lowercase()
    });
    (url, server)
}

fn ok(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
}

fn token(url: String) -> Settings {
    Settings { url, client_id: "id.access".into(), client_secret: "s3cret".into() }
}

// AC-5 and AC-6 of TRU-13: a read sends the service token as Access's two
// headers, and a User-Agent naming the client so the server can count it.
#[test]
fn a_read_sends_the_access_headers_and_names_its_client() {
    let (url, server) = serve(Some(Box::leak(ok(BOARD).into_boxed_str())));
    let snap = read(&token(url), "native").unwrap();
    assert_eq!(snap.generated_at, 7);
    let req = server.join().unwrap();
    assert!(req.starts_with("get /v1/snapshot "), "{req}");
    assert!(req.contains("\r\ncf-access-client-id: id.access\r\n"), "{req}");
    assert!(req.contains("\r\ncf-access-client-secret: s3cret\r\n"), "{req}");
    assert!(req.contains(&format!("\r\nuser-agent: sdlc-board-native/{}\r\n", env!("CARGO_PKG_VERSION"))), "{req}");
}

// AC-5 of TRU-13: each way a read fails says why.
#[test]
fn a_refused_token_says_access_refused_it() {
    let (url, _server) = serve(Some("HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"));
    let err = read(&token(url), "native").unwrap_err();
    assert_eq!(err, ReadError::Refused(401, "Unauthorized".into()));
    assert_eq!(err.to_string(), "Cloudflare Access or the board refused the service token");
}

#[test]
fn a_login_asked_for_without_a_token_says_one_is_needed() {
    let (url, _server) = serve(Some("HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"));
    let err = read(&Settings { url, ..Default::default() }, "native").unwrap_err();
    assert_eq!(err, ReadError::NeedsToken(401));
    assert_eq!(err.to_string(), "the board needs an Access service token (it answered 401)");
}

#[test]
fn a_redirect_to_the_login_page_is_a_refusal() {
    let login = "HTTP/1.1 302 Found\r\nlocation: https://x.cloudflareaccess.com/login\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
    let (url, _server) = serve(Some(login));
    assert!(matches!(read(&token(url), "native"), Err(ReadError::Refused(401, _))));
}

#[test]
fn an_answer_that_isnt_a_board_says_so() {
    let (url, _server) = serve(Some(Box::leak(ok("<html>log in</html>").into_boxed_str())));
    let err = read(&token(url), "native").unwrap_err();
    assert!(err.to_string().starts_with("the answer wasn't a board: "), "{err}");
}

#[test]
fn a_server_that_isnt_there_is_unreachable() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let err = read(&token(format!("http://127.0.0.1:{port}")), "native").unwrap_err();
    assert!(matches!(err, ReadError::Unreachable(_)), "{err:?}");
    assert!(err.to_string().starts_with("couldn't reach the board: "), "{err}");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let (url, _server) = serve(None);
    let err = read_within(&token(url), "native", Duration::from_secs(1)).unwrap_err();
    assert_eq!((err.to_string(), err), ("no answer in time".into(), ReadError::TimedOut));
}

#[test]
fn settings_that_cant_read_are_refused_before_sending() {
    let half = Settings { client_secret: String::new(), ..token("http://127.0.0.1:1".into()) };
    assert!(matches!(read(&half, "native"), Err(ReadError::Settings(_))));
}
