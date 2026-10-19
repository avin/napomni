use serde_json::json;
use std::sync::Arc;
use win_notifi_toast::{
    commands::execute_tool_once,
    reminders::{Application, CommandContext, JsonStore, SystemClock},
};
#[test]
fn replay_uses_durable_receipt_while_distinct_identical_commands_remain_allowed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(SystemClock);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(app.clock(), "Напомни через20 минут", None);
    let args =
        json!({"text":"Вода","schedule":{"kind":"after_seconds","seconds":1200}}).to_string();
    let first = execute_tool_once(
        &mut app,
        "provider-call",
        "create_reminder",
        &args,
        &context,
    )
    .unwrap();
    let repeated = execute_tool_once(
        &mut app,
        "provider-call",
        "create_reminder",
        &args,
        &context,
    )
    .unwrap();
    assert_eq!(first, repeated);
    assert_eq!(app.reminders().len(), 1);
    let mut app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(
        execute_tool_once(
            &mut app,
            "provider-call",
            "create_reminder",
            &args,
            &context
        )
        .unwrap(),
        first
    );
    assert_eq!(app.reminders().len(), 1);
    let new_context = CommandContext::capture(app.clock(), "Напомни через20 минут", None);
    execute_tool_once(
        &mut app,
        "provider-call",
        "create_reminder",
        &args,
        &new_context,
    )
    .unwrap();
    assert_eq!(app.reminders().len(), 2);
    assert!(
        execute_tool_once(
            &mut app,
            "provider-call",
            "delete_reminder",
            "{\"id\":1}",
            &context
        )
        .is_err()
    );
    assert_eq!(app.reminders().len(), 2);
}

#[test]
fn pending_typed_command_survives_restart_without_api_key_in_chat_artifacts() {
    use win_notifi_toast::{
        chat::{ChatSession, Connection, Message},
        reminder_tools::prepare_command,
    };
    let directory = tempfile::tempdir().unwrap();
    let mut chat = ChatSession::open(directory.path()).unwrap();
    chat.save_connection(Connection {
        base_url: "http://localhost/v1".into(),
        api_key: "secret-key".into(),
        model: "test".into(),
    })
    .unwrap();
    chat.record_user("Мой secret-key").unwrap();
    chat.record_assistant("Провайдер отразил secret-key")
        .unwrap();
    let context = CommandContext::capture(&SystemClock, "Исходный secret-key", Some(42));
    let identity = context.id;
    let sent = context.submitted_utc;
    let history = vec![Message {
        role: "user".into(),
        content: "Запрос secret-key".into(),
        result_for: None,
    }];
    let mut recovery = prepare_command(&history, context).unwrap();
    recovery.messages.push(
        serde_json::from_value(json!({"role":"assistant","content":"Ответ secret-key"})).unwrap(),
    );
    recovery.messages.push(serde_json::from_value(json!({"role":"assistant","content":null,"tool_calls":[{"id":"binding-1","type":"function","function":{"name":"create_reminder","arguments":json!({"text":"secret-key","metadata":{"name":"secret-key"}}).to_string()}}]})).unwrap());
    recovery.messages.push(serde_json::from_value(json!({"role":"tool","tool_call_id":"binding-1","content":json!({"name":"secret-key","nested":["secret-key"]}).to_string()})).unwrap());
    chat.save_connection(Connection {
        base_url: "http://localhost/v1".into(),
        api_key: "replacement-key".into(),
        model: "test".into(),
    })
    .unwrap();
    chat.record_assistant("Поздний ответ старого запроса: secret-key; новый replacement-key")
        .unwrap();
    chat.save_pending(Some(recovery)).unwrap();
    let mut reopened = ChatSession::open(directory.path()).unwrap();
    let pending = reopened.pending().unwrap();
    assert_eq!(pending.context.id, identity);
    assert_eq!(pending.context.selected_id, Some(42));
    assert_eq!(pending.context.submitted_utc, sent);
    assert!(
        !serde_json::to_string(pending)
            .unwrap()
            .contains("secret-key")
    );
    for file in ["chat.json", "pending-command.json"] {
        assert!(
            !std::fs::read_to_string(directory.path().join(file))
                .unwrap()
                .contains("replacement-key")
        );
        assert!(
            !std::fs::read_to_string(directory.path().join(file))
                .unwrap()
                .contains("secret-key")
        );
    }
    reopened.clear().unwrap();
    assert!(
        ChatSession::open(directory.path())
            .unwrap()
            .pending()
            .is_none()
    );
}

#[test]
fn data_and_receipt_commit_atomically_and_failed_save_can_be_retried_once() {
    use std::sync::Mutex;
    use win_notifi_toast::reminders::{Persisted, ReminderStore};
    struct Store(Arc<Mutex<(Persisted, bool)>>);
    impl ReminderStore for Store {
        fn load(&self) -> anyhow::Result<Persisted> {
            Ok(self.0.lock().unwrap().0.clone())
        }
        fn save(&self, state: &Persisted) -> anyhow::Result<()> {
            let mut stored = self.0.lock().unwrap();
            anyhow::ensure!(!stored.1, "Диск недоступен");
            stored.0 = state.clone();
            Ok(())
        }
    }
    let stored = Arc::new(Mutex::new((Persisted::default(), true)));
    let clock = Arc::new(SystemClock);
    let mut app = Application::open(clock.clone(), Box::new(Store(stored.clone()))).unwrap();
    let context = CommandContext::capture(app.clock(), "Создай", None);
    let args =
        json!({"text":"Вода","schedule":{"kind":"after_seconds","seconds":1200}}).to_string();
    assert!(execute_tool_once(&mut app, "call", "create_reminder", &args, &context).is_err());
    assert!(app.reminders().is_empty());
    stored.lock().unwrap().1 = false;
    execute_tool_once(&mut app, "call", "create_reminder", &args, &context).unwrap();
    assert_eq!(app.reminders().len(), 1);
    let mut reloaded = Application::open(clock, Box::new(Store(stored))).unwrap();
    execute_tool_once(&mut reloaded, "call", "create_reminder", &args, &context).unwrap();
    assert_eq!(reloaded.reminders().len(), 1);
}

#[test]
fn replay_does_not_repeat_pause_delete_or_undo_after_later_user_operations() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let create = CommandContext::capture(app.clock(), "Создай", None);
    let args =
        json!({"text":"Вода","schedule":{"kind":"after_seconds","seconds":1200}}).to_string();
    execute_tool_once(&mut app, "call", "create_reminder", &args, &create).unwrap();
    let pause = CommandContext::capture(app.clock(), "Пауза", Some(1));
    let paused = execute_tool_once(
        &mut app,
        "call",
        "set_reminder_paused",
        "{\"id\":1,\"paused\":true}",
        &pause,
    )
    .unwrap();
    app.undo().unwrap();
    assert!(!app.reminders()[0].paused);
    assert_eq!(
        execute_tool_once(
            &mut app,
            "call",
            "set_reminder_paused",
            "{\"id\":1,\"paused\":true}",
            &pause
        )
        .unwrap(),
        paused
    );
    assert!(!app.reminders()[0].paused);
    let delete = CommandContext::capture(app.clock(), "Удали", Some(1));
    let deleted =
        execute_tool_once(&mut app, "call", "delete_reminder", "{\"id\":1}", &delete).unwrap();
    assert!(app.reminders().is_empty());
    app.undo().unwrap();
    assert_eq!(
        execute_tool_once(&mut app, "call", "delete_reminder", "{\"id\":1}", &delete).unwrap(),
        deleted
    );
    assert_eq!(app.reminders().len(), 1);
    let edit = CommandContext::capture(app.clock(), "Без звука", Some(1));
    execute_tool_once(
        &mut app,
        "edit",
        "update_reminder",
        "{\"id\":1,\"patch\":{\"sound\":false}}",
        &edit,
    )
    .unwrap();
    let undo = CommandContext::capture(app.clock(), "Отмени", None);
    execute_tool_once(&mut app, "undo", "undo_last_operation", "{}", &undo).unwrap();
    assert!(app.reminders()[0].sound);
    app.set_paused(1, true).unwrap();
    execute_tool_once(&mut app, "undo", "undo_last_operation", "{}", &undo).unwrap();
    assert!(app.reminders()[0].paused);
}

#[test]
fn rejected_tool_calls_leave_records_and_last_undo_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(app.clock(), "Создай", None);
    execute_tool_once(
        &mut app,
        "create",
        "create_reminder",
        &json!({"text":"Вода","schedule":{"kind":"after_seconds","seconds":1200}}).to_string(),
        &context,
    )
    .unwrap();
    let before = app.reminders().to_vec();
    for (id, name, args) in [
        ("bad-json", "create_reminder", "{"),
        (
            "unknown",
            "update_reminder",
            "{\"id\":999,\"patch\":{\"sound\":false}}",
        ),
        (
            "unsupported",
            "create_reminder",
            "{\"text\":\"Нельзя\",\"schedule\":{\"kind\":\"business_holidays\"}}",
        ),
        (
            "invalid",
            "update_reminder",
            "{\"id\":1,\"patch\":{\"text\":\"\"}}",
        ),
    ] {
        assert!(execute_tool_once(&mut app, id, name, args, &context).is_err());
        assert_eq!(app.reminders(), before.as_slice());
    }
    app.undo().unwrap();
    assert!(app.reminders().is_empty());
}
