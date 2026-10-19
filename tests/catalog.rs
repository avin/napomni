use napomni::{
    catalog::{Catalog, ToolSupport},
    chat::{ChatSession, Connection},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
fn fixture(status: &str, body: Value) -> (Connection, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 2048];
        while !bytes.windows(4).any(|s| s == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let request = String::from_utf8_lossy(&bytes);
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer fixture-key")
        );
        let body = body.to_string();
        write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    (
        Connection {
            base_url: format!("http://{address}/v1"),
            api_key: "fixture-key".into(),
            model: "manual-old".into(),
        },
        task,
    )
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}
#[test]
fn large_catalog_searches_locally_and_selection_survives_restart() {
    let mut data: Vec<Value> = (0..2400)
        .map(|n| json!({"id":format!("provider/model-{n}"),"name":format!("Модель {n}")}))
        .collect();
    data.push(json!({"id":"vendor/needle","name":"Особая Модель"}));
    let (connection, server) = fixture("200 OK", json!({"data":data}));
    let catalog = runtime().block_on(Catalog::load(&connection)).unwrap();
    server.join().unwrap();
    assert_eq!(catalog.search("").len(), 2401);
    assert_eq!(catalog.search("NEEDle")[0].id, "vendor/needle");
    assert_eq!(catalog.search("ОСОБАЯ")[0].id, "vendor/needle");
    assert_eq!(catalog.search("model-2399")[0].id, "provider/model-2399");
    assert!(catalog.search("missing-result").is_empty());
    assert_eq!(
        catalog.search("needle")[0].tool_support,
        ToolSupport::Unknown
    );
    let directory = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(directory.path()).unwrap();
    chat.save_connection(connection).unwrap();
    catalog.select("vendor/needle", &mut chat).unwrap();
    assert_eq!(
        ChatSession::open(directory.path())
            .unwrap()
            .connection()
            .model,
        "vendor/needle"
    );
}
#[test]
fn openrouter_tools_metadata_distinguishes_supported_unknown_and_unsupported() {
    let (connection, server) = fixture(
        "200 OK",
        json!({"data":[
         {"id":"vendor/tools","name":"Tools Model","supported_parameters":["temperature","tools","tool_choice"]},
         {"id":"vendor/unknown"},
         {"id":"vendor/plain","supported_parameters":["temperature"]},
         {"id":"vendor/empty","supported_parameters":[]}
        ]}),
    );
    let catalog = runtime().block_on(Catalog::load(&connection)).unwrap();
    server.join().unwrap();
    assert_eq!(catalog.search("").len(), 2);
    assert_eq!(
        catalog.search("TOOLS")[0].tool_support,
        ToolSupport::Supported
    );
    assert_eq!(
        catalog.search("unknown")[0].tool_support,
        ToolSupport::Unknown
    );
    assert!(catalog.search("plain").is_empty());
    let directory = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(directory.path()).unwrap();
    chat.save_connection(connection).unwrap();
    assert!(catalog.select("vendor/plain", &mut chat).is_err());
    assert_eq!(chat.connection().model, "manual-old");
}
#[test]
fn missing_catalog_keeps_manual_model_available_and_does_not_leak_key() {
    let (connection, server) = fixture(
        "404 Not Found",
        json!({"error":{"message":"fixture-key","code":"not_found"}}),
    );
    let directory = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(directory.path()).unwrap();
    chat.save_connection(connection.clone()).unwrap();
    let error = match runtime().block_on(Catalog::load(&connection)) {
        Ok(_) => panic!("Catalog must be unavailable"),
        Err(error) => error,
    };
    server.join().unwrap();
    assert!(!error.to_string().contains("fixture-key"));
    assert!(error.to_string().contains("вручную"));
    assert_eq!(chat.connection().model, "manual-old");
    let mut manual = chat.connection().clone();
    manual.model = "manual-new".into();
    chat.save_connection(manual).unwrap();
    assert_eq!(
        ChatSession::open(directory.path())
            .unwrap()
            .connection()
            .model,
        "manual-new"
    );
}
#[test]
fn selected_catalog_id_is_used_in_subsequent_chat_request() {
    let (connection, server) = fixture(
        "200 OK",
        json!({"data":[{"id":"vendor/chosen","name":"Chosen"}]}),
    );
    let catalog = runtime().block_on(Catalog::load(&connection)).unwrap();
    server.join().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        let offset;
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                offset = i + 4;
                break;
            }
        }
        let header = String::from_utf8_lossy(&bytes[..offset]);
        assert!(header.starts_with("POST /v1/chat/completions"));
        let length: usize = header
            .lines()
            .find_map(|s| {
                s.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < offset + length {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let body: Value = serde_json::from_slice(&bytes[offset..]).unwrap();
        assert_eq!(body["model"], "vendor/chosen");
        let reply=json!({"id":"reply","object":"chat.completion","created":1,"model":"vendor/chosen","choices":[{"index":0,"message":{"role":"assistant","content":"Ответ выбранной модели"},"finish_reason":"stop"}]}).to_string();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len()).unwrap();
    });
    let directory = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(directory.path()).unwrap();
    chat.save_connection(Connection {
        base_url: format!("http://{address}/v1"),
        ..connection
    })
    .unwrap();
    catalog.select("vendor/chosen", &mut chat).unwrap();
    let reopened = ChatSession::open(directory.path()).unwrap();
    let provider = napomni::chat::Provider::new(reopened.connection()).unwrap();
    let reply = runtime()
        .block_on(provider.reply(&[napomni::chat::Message {
            role: "user".into(),
            content: "Привет".into(),
            result_for: None,
        }]))
        .unwrap();
    assert_eq!(reply, "Ответ выбранной модели");
    task.join().unwrap();
}
