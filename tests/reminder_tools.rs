use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};
use win_notifi_toast::{
    chat::{Connection, Message, Provider},
    reminder_tools::{run_command, run_command_with_progress},
    reminders::{Application, CommandContext, JsonStore, SystemClock},
};
fn completion(message: Value) -> Value {
    json!({"id":"fixture","object":"chat.completion","created":1,"model":"fixture","choices":[{"index":0,"message":message,"finish_reason":"stop"}]})
}
fn server(responses: Vec<Value>) -> (Provider, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let provider = Provider::new(&Connection {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        api_key: "fixture".into(),
        model: "fixture".into(),
    })
    .unwrap();
    let handle = thread::spawn(move || {
        let mut requests = vec![];
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 4096];
            let offset = loop {
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let header = String::from_utf8_lossy(&bytes[..offset]);
            let len: usize = header
                .lines()
                .find_map(|s| {
                    s.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < offset + len {
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            requests.push(serde_json::from_slice(&bytes[offset..offset + len]).unwrap());
            let body = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (provider, handle)
}
#[test]
fn tool_creates_saved_reminder_and_returns_actual_result_to_model() {
    let (provider, server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"create1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Попить воды","schedule":{"kind":"after_seconds","seconds":1200}}).to_string()}}]}),
        ),
        completion(json!({"role":"assistant","content":"Готово"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let app = Arc::new(Mutex::new(
        Application::open(
            clock.clone(),
            Box::new(JsonStore::new(dir.path().join("data.json"))),
        )
        .unwrap(),
    ));
    let context =
        CommandContext::capture(clock.as_ref(), "Напомни попить воды через20 минут", None);
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
    }];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_none());
    assert_eq!(app.lock().unwrap().reminders()[0].text, "Попить воды");
    assert!(outcome.facts[0].contains("Попить воды"));
    let requests = server.join().unwrap();
    assert_eq!(requests[1]["messages"][3]["role"], "tool");
    let result: Value =
        serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["reminder"]["id"], 1);
    assert_eq!(result["reminder"]["text"], "Попить воды");
}

#[test]
fn ordinary_prose_and_invalid_tool_arguments_do_not_change_reminders() {
    for arguments in [
        None,
        Some("{\"text\":\"Нельзя\",\"schedule\":{\"kind\":\"monthly\"}}"),
        Some("{bad json"),
    ] {
        let mut responses = vec![];
        if let Some(arguments) = arguments {
            responses.push(completion(json!({"role":"assistant","content":null,"tool_calls":[{"id":"invalid","type":"function","function":{"name":"create_reminder","arguments":arguments}}]})));
        }
        responses.push(completion(json!({"role":"assistant","content":"Создано"})));
        let (provider, server) = server(responses);
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(SystemClock);
        let app = Arc::new(Mutex::new(
            Application::open(
                clock.clone(),
                Box::new(JsonStore::new(dir.path().join("data.json"))),
            )
            .unwrap(),
        ));
        let context = CommandContext::capture(clock.as_ref(), "Неполная команда", None);
        let history = vec![Message {
            role: "user".into(),
            content: context.source.clone(),
        }];
        let outcome = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_command(&provider, &history, context, app.clone()));
        assert!(outcome.facts.is_empty());
        assert!(app.lock().unwrap().reminders().is_empty());
        let requests = server.join().unwrap();
        if arguments.is_some() {
            let result: Value =
                serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap())
                    .unwrap();
            assert_eq!(result["ok"], false);
        }
    }
}

#[test]
fn committed_tool_result_survives_failure_of_final_model_response() {
    let (provider, server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"committed","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Сохранено","schedule":{"kind":"after_seconds","seconds":1200}}).to_string()}}]}),
        ),
        json!({"not":"a valid completion"}),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let path = dir.path().join("data.json");
    let app = Arc::new(Mutex::new(
        Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    let mut progress = vec![];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command_with_progress(
            &provider,
            &[],
            context,
            app.clone(),
            |fact| progress.push(fact.to_owned()),
        ));
    assert!(outcome.error.is_some());
    assert_eq!(progress.len(), 1);
    assert_eq!(outcome.facts, progress);
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    assert_eq!(
        Application::open(clock, Box::new(JsonStore::new(&path)))
            .unwrap()
            .reminders()
            .len(),
        1
    );
    server.join().unwrap();
}
