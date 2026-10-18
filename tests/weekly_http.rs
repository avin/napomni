use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};
use win_notifi_toast::{
    chat::{Connection, Message, Provider},
    reminder_tools::run_command,
    reminders::{Application, Clock, CommandContext, JsonStore, TimeSample},
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

struct TestClock(Mutex<TimeSample>);
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.naive_utc()
    }
    fn resolve_local(&self, at: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![at.and_utc()]
    }
}
#[test]
fn weekly_command_reaches_provider_saves_returns_description_and_fires_offline() {
    let arguments = json!({"text":"Размяться","schedule":{"kind":"weekly","weekdays":[2,4],"times":["09:00:00","16:00:00"],"every_n_weeks":2,"anchor_monday":"2026-10-19"}});
    let (provider, server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"weekly1","type":"function","function":{"name":"create_reminder","arguments":arguments.to_string()}}]}),
        ),
        completion(json!({"role":"assistant","content":"Сохранено"})),
    ]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-19T08:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let app = Arc::new(Mutex::new(
        Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let context = CommandContext::capture(
        clock.as_ref(),
        "Напомни размяться раз в две недели по вторникам и четвергам в 9 и 16, начиная с недели 19 октября",
        None,
    );
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
    }];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_none());
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
    assert!(outcome.facts[0].contains("вт, чт"));
    assert!(outcome.facts[0].contains("09:00:00, 16:00:00"));
    let requests = server.join().unwrap();
    let tools = &requests[0]["tools"];
    assert!(
        tools.to_string().contains("anchor_monday"),
        "provider must receive the weekly rule schema"
    );
    let result: Value =
        serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["ok"], true);
    assert!(
        result["schedule_description"]
            .as_str()
            .unwrap()
            .contains("2 нед.")
    );
    drop(app);
    let mut offline = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    {
        let mut now = clock.0.lock().unwrap();
        now.utc += Duration::hours(25);
        now.awake_millis += 25 * 3_600_000;
    }
    offline.tick().unwrap();
    assert_eq!(offline.open_toasts()[0].text, "Размяться");
}
