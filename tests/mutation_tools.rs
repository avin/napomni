use napomni::{
    reminder_tools::execute_tool,
    reminders::{
        Application, CommandContext, CreateReminder, JsonStore, ScheduleInput, SystemClock,
    },
};
use serde_json::json;
use std::sync::Arc;
#[test]
fn tools_patch_selected_id_and_return_saved_properties_without_changing_others() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let mut app = Application::open(
        clock,
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let create = CommandContext::capture(app.clock(), "Создай", None);
    let reminder = app
        .create(
            CreateReminder {
                text: "Текст".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 3600 },
                border: None,
                sound: None,
            },
            &create,
        )
        .unwrap();
    let context = CommandContext::capture(app.clock(), "Выключи звук", Some(reminder.id));
    let result = execute_tool(
        &mut app,
        "update_reminder",
        &json!({"id":reminder.id,"patch":{"sound":false}}).to_string(),
        &context,
    )
    .unwrap();
    assert_eq!(result["reminder"]["sound"], false);
    assert_eq!(result["reminder"]["text"], "Текст");
    assert_eq!(app.reminders()[0].schedule, reminder.schedule);
    assert_eq!(app.reminders()[0].border, reminder.border);
    assert!(
        execute_tool(
            &mut app,
            "delete_reminder",
            &json!({"id":reminder.id+1}).to_string(),
            &context
        )
        .is_err()
    );
    assert_eq!(app.reminders().len(), 1);
    execute_tool(&mut app, "undo_last_operation", "{}", &context).unwrap();
    assert!(app.reminders()[0].sound);
}
