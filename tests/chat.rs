use win_notifi_toast::chat::{ChatSession, Connection};
#[test]
fn settings_and_conversation_survive_restart_and_clear() {
    let folder = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(folder.path()).unwrap();
    chat.save_connection(Connection {
        base_url: "http://localhost:9999/v1".into(),
        api_key: "secret".into(),
        model: "test".into(),
    })
    .unwrap();
    chat.record_user("Привет").unwrap();
    let mut reopened = ChatSession::open(folder.path()).unwrap();
    assert_eq!(reopened.messages()[0].content, "Привет");
    assert_eq!(reopened.connection().api_key, "secret");
    reopened.clear().unwrap();
    assert!(
        ChatSession::open(folder.path())
            .unwrap()
            .messages()
            .is_empty()
    );
}

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use win_notifi_toast::chat::{ConnectionError, Provider};
fn serve_once(status: &str, body: Value) -> (Connection, thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let offset;
        loop {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                offset = i + 4;
                break;
            }
        }
        let header = String::from_utf8_lossy(&bytes[..offset]);
        assert!(header.starts_with("POST /v1/chat/completions"));
        assert!(
            header
                .to_lowercase()
                .contains("authorization: bearer test-key")
        );
        let length: usize = header
            .lines()
            .find_map(|s| {
                s.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|n| n.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < offset + length {
            let n = stream.read(&mut buffer).unwrap();
            bytes.extend_from_slice(&buffer[..n]);
        }
        let body = body.to_string();
        write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        serde_json::from_slice(&bytes[offset..]).unwrap()
    });
    (
        Connection {
            base_url: format!("http://{address}/v1"),
            api_key: "test-key".into(),
            model: "fixture".into(),
        },
        handle,
    )
}
fn completion(message: Value) -> Value {
    json!({"id":"test","object":"chat.completion","created":1,"model":"fixture","choices":[{"index":0,"message":message,"finish_reason":"stop"}]})
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}
#[test]
fn ordinary_reply_does_not_force_tools() {
    let (connection, server) = serve_once(
        "200 OK",
        completion(json!({"role":"assistant","content":"Здравствуйте!"})),
    );
    let reply =
        runtime()
            .block_on(Provider::new(&connection).unwrap().reply(&[
                win_notifi_toast::chat::Message {
                    role: "user".into(),
                    content: "Привет".into(),
                },
            ]))
            .unwrap();
    assert_eq!(reply, "Здравствуйте!");
    assert_eq!(server.join().unwrap()["tool_choice"], "auto");
}
#[test]
fn probe_requires_an_actual_safe_tool_call() {
    let (connection, server) = serve_once(
        "200 OK",
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"probe1","type":"function","function":{"name":"connection_probe","arguments":"{\"ok\":true}"}}]}),
        ),
    );
    runtime()
        .block_on(Provider::new(&connection).unwrap().probe())
        .unwrap();
    assert_eq!(
        server.join().unwrap()["tool_choice"]["function"]["name"],
        "connection_probe"
    );
    let (connection, server) = serve_once(
        "200 OK",
        completion(json!({"role":"assistant","content":"Готово"})),
    );
    assert_eq!(
        runtime().block_on(Provider::new(&connection).unwrap().probe()),
        Err(ConnectionError::Tools)
    );
    server.join().unwrap();
}
#[test]
fn provider_failures_are_actionable_without_echoing_secrets() {
    for (code, expected) in [
        ("invalid_api_key", ConnectionError::Authorization),
        ("model_not_found", ConnectionError::Model),
    ] {
        let (connection, server) = serve_once(
            "400 Bad Request",
            json!({"error":{"message":"test-key","type":"invalid_request_error","code":code}}),
        );
        let error = runtime()
            .block_on(Provider::new(&connection).unwrap().probe())
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("test-key"));
        server.join().unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let c = Connection {
        base_url: format!("http://{address}/v1"),
        api_key: "test-key".into(),
        model: "fixture".into(),
    };
    assert_eq!(
        runtime().block_on(Provider::new(&c).unwrap().probe()),
        Err(ConnectionError::Network)
    );
    let c = Connection {
        base_url: "not a url".into(),
        ..c
    };
    assert!(matches!(Provider::new(&c), Err(ConnectionError::Url)));
}
