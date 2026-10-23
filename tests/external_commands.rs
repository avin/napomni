use napomni::{external_commands::execute, reminder_tools::execute_tool, reminders::{Application, CommandContext, JsonStore, SystemClock}};
use serde_json::json;
use std::sync::Arc;

#[test]
fn external_list_contains_reminders_created_by_chat() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = Application::open(Arc::new(SystemClock), Box::new(JsonStore::new(dir.path().join("reminders.json")))).unwrap();
    let context = CommandContext::capture(app.clock(), "chat", None);
    execute_tool(&mut app, "create_reminder", &json!({"text":"Чат: \"вода\"\nещё", "schedule":{"kind":"after_seconds","seconds":3600}}).to_string(), &context).unwrap();
    let result = execute(&mut app, "list", &json!({})).unwrap();
    assert_eq!(result["reminders"][0]["reminder"]["text"], "Чат: \"вода\"\nещё");
    assert_eq!(result["reminders"][0]["reminder"]["id"], 1);
    assert!(result["reminders"][0]["schedule_description"].is_string());
    assert!(result["reminders"][0].get("schedule_status").is_some());
}
