use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};
use win_notifi_toast::{
    chat::{Connection, Message, Provider},
    reminder_tools::{resume_command, run_command},
    reminders::{Application, CommandContext, JsonStore, SystemClock},
};
fn completion(message: Value) -> Value {
    json!({"id":"test","object":"chat.completion","created":1,"model":"test","choices":[{"index":0,"message":message,"finish_reason":"stop"}]})
}
fn server(responses: Vec<Value>) -> (Provider, thread::JoinHandle<Vec<Value>>) {
    server_gated(responses, None)
}
struct Gate {
    at: usize,
    reached: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
fn server_gated(
    responses: Vec<Value>,
    gate: Option<Gate>,
) -> (Provider, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let provider = Provider::new(&Connection {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        api_key: "secret-key".into(),
        model: "test".into(),
    })
    .unwrap();
    let handle = thread::spawn(move || {
        let mut requests = vec![];
        for (index, response) in responses.into_iter().enumerate() {
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
                        .map(|n| n.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < offset + len {
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            requests.push(serde_json::from_slice(&bytes[offset..offset + len]).unwrap());
            if let Some(gate) = &gate
                && gate.at == index
            {
                gate.reached.send(()).unwrap();
                gate.release
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
            let body = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (provider, handle)
}

#[test]
fn recovery_hydrates_committed_receipt_after_crash_before_transcript_was_saved() {
    use win_notifi_toast::{commands::execute_tool_once, reminder_tools::prepare_command};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.json");
    let clock = Arc::new(SystemClock);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(app.clock(), "Создай", None);
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
    }];
    let initial_pending = prepare_command(&history, context.clone()).unwrap();
    execute_tool_once(
        &mut app,
        "durable-call",
        "create_reminder",
        &json!({"text":"Сохранено до сбоя","schedule":{"kind":"after_seconds","seconds":1200}})
            .to_string(),
        &context,
    )
    .unwrap();
    let app = Arc::new(Mutex::new(
        Application::open(clock, Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let (provider, server) = server(vec![completion(
        json!({"role":"assistant","content":"Запись сохранена"}),
    )]);
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(resume_command(&provider, initial_pending, app.clone()));
    assert!(outcome.error.is_none());
    assert_eq!(outcome.facts.len(), 1);
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    let requests = server.join().unwrap();
    let messages = requests[0]["messages"].as_array().unwrap();
    assert_eq!(messages[2]["tool_calls"][0]["id"], "durable-call");
    assert_eq!(messages[3]["tool_call_id"], "durable-call");
    let result: Value = serde_json::from_str(messages[3]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["reminder"]["text"], "Сохранено до сбоя");
}

#[test]
fn clarification_preserves_selected_id_and_follow_up_changes_only_the_named_property() {
    use win_notifi_toast::reminders::{CreateReminder, ScheduleInput};
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let mut core = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    for _ in 0..2 {
        core.create(
            CreateReminder {
                text: "Одинаковый текст".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 1200 },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap();
    }
    let originals = core.reminders().to_vec();
    let app = Arc::new(Mutex::new(core));
    let selected = CommandContext::capture(clock.as_ref(), "Измени цвет", Some(2));
    let mut history = vec![Message {
        role: "user".into(),
        content: selected.source.clone(),
    }];
    let (provider, first_server) = server(vec![completion(
        json!({"role":"assistant","content":"Какой цвет?"}),
    )]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let clarification = runtime.block_on(run_command(&provider, &history, selected, app.clone()));
    assert!(clarification.facts.is_empty());
    assert_eq!(app.lock().unwrap().reminders(), originals.as_slice());
    let first_requests = first_server.join().unwrap();
    assert!(
        first_requests[0]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Some(2)")
    );
    history.push(Message {
        role: "assistant".into(),
        content: clarification.text,
    });
    history.push(Message {
        role: "user".into(),
        content: "Зелёный".into(),
    });
    let (provider, second_server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"edit","type":"function","function":{"name":"update_reminder","arguments":"{\"id\":2,\"patch\":{\"border\":65280}}"}}]}),
        ),
        completion(json!({"role":"assistant","content":"Готово"})),
    ]);
    let follow_up = CommandContext::capture(clock.as_ref(), "Зелёный", Some(2));
    let outcome = runtime.block_on(run_command(&provider, &history, follow_up, app.clone()));
    assert!(outcome.error.is_none());
    let app = app.lock().unwrap();
    assert_eq!(app.reminders()[0], originals[0]);
    let mut expected = originals[1].clone();
    expected.border = 65280;
    assert_eq!(app.reminders()[1], expected);
    second_server.join().unwrap();
}

#[test]
fn waiting_for_failed_api_response_does_not_hold_core_or_stop_offline_schedule() {
    use chrono::{DateTime, Duration, NaiveDateTime, Utc};
    use win_notifi_toast::reminders::{Clock, CreateReminder, ScheduleInput, TimeSample};
    struct TestClock(Mutex<TimeSample>);
    impl Clock for TestClock {
        fn sample(&self) -> TimeSample {
            *self.0.lock().unwrap()
        }
        fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
            utc.naive_utc()
        }
        fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
            vec![local.and_utc()]
        }
    }
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-18T09:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let directory = tempfile::tempdir().unwrap();
    let mut core = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let id = core
        .create(
            CreateReminder {
                text: "Офлайн".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 60 },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap()
        .id;
    let app = Arc::new(Mutex::new(core));
    let (reached_sender, reached) = std::sync::mpsc::channel();
    let (release_sender, release) = std::sync::mpsc::channel();
    let (provider, server) = server_gated(
        vec![json!({"invalid":"completion"})],
        Some(Gate {
            at: 0,
            reached: reached_sender,
            release,
        }),
    );
    let worker_app = app.clone();
    let worker = thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_command(&provider, &[], context, worker_app))
    });
    reached
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    {
        let mut now = clock.0.lock().unwrap();
        now.utc += Duration::seconds(60);
        now.awake_millis += 60_000;
    }
    {
        let mut core = app
            .try_lock()
            .expect("network await must release application lock");
        core.tick().unwrap();
        assert_eq!(core.open_toasts()[0].reminder_id, id);
    }
    release_sender.send(()).unwrap();
    let outcome = worker.join().unwrap();
    assert!(outcome.error.is_some());
    assert!(outcome.facts.is_empty());
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    server.join().unwrap();
}
#[test]
fn failed_final_response_resumes_with_saved_tool_result_instead_of_executing_again() {
    let create = json!({"id":"call1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Вода","schedule":{"kind":"after_seconds","seconds":1200}}).to_string()}});
    let (provider, first_server) = server(vec![
        completion(json!({"role":"assistant","content":null,"tool_calls":[create]})),
        json!({"broken":true}),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.json");
    let clock = Arc::new(SystemClock);
    let app = Arc::new(Mutex::new(
        Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let context = CommandContext::capture(clock.as_ref(), "Напомни через20 минут", None);
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
    }];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let outcome = runtime.block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_some());
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    assert_eq!(outcome.facts.len(), 1);
    first_server.join().unwrap();
    let recovery =
        serde_json::from_value(serde_json::to_value(outcome.recovery.unwrap()).unwrap()).unwrap();
    let app = Arc::new(Mutex::new(
        Application::open(clock, Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let (provider, second_server) = server(vec![completion(
        json!({"role":"assistant","content":"Результат сохранён"}),
    )]);
    let recovered = runtime.block_on(resume_command(&provider, recovery, app.clone()));
    assert!(recovered.error.is_none());
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    let requests = second_server.join().unwrap();
    let messages = requests[0]["messages"].as_array().unwrap();
    assert_eq!(messages.last().unwrap()["role"], "tool");
    assert_eq!(messages.last().unwrap()["tool_call_id"], "call1");
}
