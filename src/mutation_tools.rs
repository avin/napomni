use crate::{
    mutations::ReminderPatch,
    reminders::{Application, CommandContext, Reminder, ReminderId},
};
use anyhow::{Result, bail, ensure};
use async_openai::types::ChatCompletionTool;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn tools(schedule: Value) -> Vec<ChatCompletionTool> {
    serde_json::from_value(json!([
        {"type":"function","function":{"name":"update_reminder","description":"Update the selected reminder by ID. Include only explicitly requested properties in patch; all others are preserved.","parameters":{"type":"object","properties":{"id":{"type":"integer","minimum":1},"patch":{"type":"object","properties":{"text":{"type":"string"},"schedule":schedule,"limits":crate::constraints::schema(),"border":{"type":"integer","minimum":0,"maximum":16777215},"sound":{"type":"boolean"}},"minProperties":1,"additionalProperties":false}},"required":["id","patch"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"set_reminder_paused","description":"Pause or resume future occurrences of the selected ID. An already open toast remains unchanged.","parameters":{"type":"object","properties":{"id":{"type":"integer","minimum":1},"paused":{"type":"boolean"}},"required":["id","paused"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"delete_reminder","description":"Delete a reminder by ID. An already open toast remains unchanged. Deletion can be undone.","parameters":{"type":"object","properties":{"id":{"type":"integer","minimum":1}},"required":["id"],"additionalProperties":false}}},
        {"type":"function","function":{"name":"undo_last_operation","description":"Undo the last create, update, pause, or delete operation. Does not restore closed toasts.","parameters":{"type":"object","properties":{},"additionalProperties":false}}}
    ])).expect("static mutation tools")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    id: ReminderId,
    patch: ReminderPatch,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pause {
    id: ReminderId,
    paused: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    id: ReminderId,
}
fn target(id: ReminderId, context: &CommandContext) -> Result<()> {
    ensure!(
        context.selected_id.is_none_or(|selected| selected == id),
        "Выбрана другая запись. Уточните ID напоминания"
    );
    Ok(())
}
fn result(app: &Application, reminder: Reminder, action: &str) -> Value {
    let description = reminder.schedule.description(app.clock());
    json!({"ok":true,"reminder":reminder,"schedule_description":description,"fact":format!("{action} #{}: «{}». {}. Звук: {}; {}",reminder.id,reminder.text,description,if reminder.sound {"включён"} else {"выключен"},if reminder.paused {"приостановлено"} else {"не приостановлено"})})
}
pub(crate) fn execute(
    app: &mut Application,
    name: &str,
    arguments: &str,
    context: &CommandContext,
) -> Result<Value> {
    match name {
        "update_reminder" => {
            let args: Update = serde_json::from_str(arguments)
                .map_err(|_| anyhow::anyhow!("Неверные аргументы изменения напоминания"))?;
            target(args.id, context)?;
            let reminder = app.update(args.id, args.patch, context)?;
            Ok(result(app, reminder, "Изменено"))
        }
        "set_reminder_paused" => {
            let args: Pause = serde_json::from_str(arguments)
                .map_err(|_| anyhow::anyhow!("Укажите ID и состояние паузы"))?;
            target(args.id, context)?;
            let reminder = app.set_paused(args.id, args.paused)?;
            Ok(result(
                app,
                reminder,
                if args.paused {
                    "Приостановлено"
                } else {
                    "Возобновлено"
                },
            ))
        }
        "delete_reminder" => {
            let args: Delete = serde_json::from_str(arguments)
                .map_err(|_| anyhow::anyhow!("Укажите ID для удаления"))?;
            target(args.id, context)?;
            let reminder = app.delete(args.id)?;
            Ok(
                json!({"ok":true,"deleted_id":reminder.id,"fact":format!("Удалено #{}: «{}»",reminder.id,reminder.text)}),
            )
        }
        "undo_last_operation" => {
            ensure!(
                serde_json::from_str::<Value>(arguments)? == json!({}),
                "Отмена не принимает аргументы"
            );
            match app.undo()? {
                Some(reminder) => Ok(result(app, reminder, "Отмена выполнена. Восстановлено")),
                None => Ok(json!({"ok":true,"fact":"Отмена выполнена. Созданная запись удалена"})),
            }
        }
        _ => bail!("Неизвестный инструмент. Данные не изменены"),
    }
}
