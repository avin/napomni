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
        result_for: None,
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
            result_for: None,
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
#[test]
fn interval_chat_exposes_schema_and_returns_durable_grid_facts() {
    let (provider, server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"interval1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Вода","schedule":{"kind":"interval","seconds":600}}).to_string()}}]}),
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
        CommandContext::capture(clock.as_ref(), "Напоминай пить воду каждые 10 минут", None);
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
        result_for: None,
    }];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_none());
    assert!(outcome.facts[0].contains("10 мин"));
    let requests = server.join().unwrap();
    let variants = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "create_reminder")
        .unwrap()["function"]["parameters"]["properties"]["schedule"]["oneOf"]
        .as_array()
        .unwrap();
    assert!(
        variants
            .iter()
            .any(|variant| variant["properties"]["kind"]["const"] == "interval")
    );
    let result: Value =
        serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["reminder"]["schedule"]["kind"], "interval");
    assert_eq!(result["reminder"]["schedule"]["period_seconds"], 600);
    assert_eq!(app.lock().unwrap().reminders().len(), 1);
}
#[test]
fn calendar_date_chat_tools_publish_actual_monthly_and_yearly_rules() {
    let (provider, server) = server(vec![
        completion(json!({"role":"assistant","content":null,"tool_calls":[
         {"id":"monthly1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Месячный отчёт","schedule":{"kind":"monthly","days":[1,31],"times":["12:00:00","18:00:00"]}}).to_string()}},
         {"id":"yearly1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Високосная дата","schedule":{"kind":"yearly","dates":[{"month":2,"day":29}],"times":["09:00:00"]}}).to_string()}}
        ]})),
        completion(json!({"role":"assistant","content":"Сохранено"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let path = dir.path().join("data.json");
    let app = Arc::new(Mutex::new(
        Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap(),
    ));
    let context = CommandContext::capture(clock.as_ref(), "Каждое 1 и 31 число и 29 февраля", None);
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
        result_for: None,
    }];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_none());
    assert_eq!(outcome.facts.len(), 2);
    assert!(outcome.facts[0].contains("Ежемесячно"));
    assert!(outcome.facts[1].contains("Ежегодно"));
    let requests = server.join().unwrap();
    let variants = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "create_reminder")
        .unwrap()["function"]["parameters"]["properties"]["schedule"]["oneOf"]
        .as_array()
        .unwrap();
    for kind in ["monthly", "yearly", "weekly", "interval", "after_seconds"] {
        assert!(
            variants
                .iter()
                .any(|variant| variant["properties"]["kind"]["const"] == kind),
            "Missing schedule schema: {kind}"
        );
    }
    let monthly: Value =
        serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
    let yearly: Value =
        serde_json::from_str(requests[1]["messages"][4]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        monthly["reminder"]["schedule"]["rule"]["days"],
        json!([1, 31])
    );
    assert_eq!(
        yearly["reminder"]["schedule"]["rule"]["dates"],
        json!([{"month":2,"day":29}])
    );
    let reopened = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(reopened.reminders().len(), 2);
    assert!(reopened.open_toasts().is_empty());
}
#[test]
fn constrained_chat_supports_workday_window_exclusion_and_limits_only_clear() {
    use chrono::Days;
    use win_notifi_toast::reminders::Clock;
    let clock = Arc::new(SystemClock);
    let today = clock.local_at(clock.sample().utc).date();
    let end = today.checked_add_days(Days::new(30)).unwrap();
    let excluded = today.checked_add_days(Days::new(1)).unwrap();
    let limits = json!({"start_date":today.to_string(),"end_date":end.to_string(),"window":{"start":"09:00:00","end":"18:00:00"},"weekdays":[1,2,3,4,5],"excluded_dates":[excluded.to_string()]});
    let (provider, server) = server(vec![
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"limited1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"Разминка","schedule":{"kind":"constrained","base":{"kind":"interval","seconds":1800},"limits":limits}}).to_string()}}]}),
        ),
        completion(
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"clear1","type":"function","function":{"name":"update_reminder","arguments":json!({"id":1,"patch":{"limits":{}}}).to_string()}}]}),
        ),
        completion(json!({"role":"assistant","content":"Готово"})),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let app = Arc::new(Mutex::new(
        Application::open(
            clock.clone(),
            Box::new(JsonStore::new(dir.path().join("data.json"))),
        )
        .unwrap(),
    ));
    let context = CommandContext::capture(
        clock.as_ref(),
        "Каждые полчаса с 9 до 18 по будням, кроме выбранной даты",
        None,
    );
    let history = vec![Message {
        role: "user".into(),
        content: context.source.clone(),
        result_for: None,
    }];
    let outcome = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_command(&provider, &history, context, app.clone()));
    assert!(outcome.error.is_none());
    assert_eq!(outcome.facts.len(), 2);
    assert!(outcome.facts[0].contains("09:00:00–18:00:00 включительно"));
    assert!(outcome.facts[0].contains("пн, вт, ср, чт, пт"));
    assert!(outcome.facts[0].contains(&excluded.format("%d.%m.%Y").to_string()));
    let requests = server.join().unwrap();
    let create = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "create_reminder")
        .unwrap();
    assert!(
        create["function"]["parameters"]["properties"]["schedule"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|variant| variant["properties"]["kind"]["const"] == "constrained")
    );
    let update = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "update_reminder")
        .unwrap();
    assert_eq!(
        update["function"]["parameters"]["properties"]["patch"]["properties"]["limits"]["type"],
        "object"
    );
    let before: Value =
        serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
    let after: Value =
        serde_json::from_str(requests[2]["messages"][5]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        before["reminder"]["schedule"]["base"],
        after["reminder"]["schedule"]["base"]
    );
    assert_eq!(
        after["reminder"]["schedule"]["limits"]["window"],
        Value::Null
    );
}
