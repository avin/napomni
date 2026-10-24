//! Local commands use the same application and history as the internal chat.
use crate::{
    reminder_tools::execute_tool,
    reminders::{Application, CommandContext},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReminderTarget {
    id: crate::reminders::ReminderId,
}

/// The CLI's argument schemas come from the same definitions the chat receives.
pub fn schemas() -> Value {
    let mut schemas = serde_json::Map::new();
    for tool in crate::reminder_tools::tools() {
        let command = match tool.function.name.as_str() {
            "list_reminders" => "list",
            "create_reminder" => "create",
            "update_reminder" => "update",
            "delete_reminder" => "delete",
            "undo_last_operation" => "undo",
            _ => continue,
        };
        schemas.insert(
            command.into(),
            tool.function.parameters.unwrap_or(json!({})),
        );
    }
    let target = schemas["delete"].clone();
    schemas.insert("pause".into(), target.clone());
    schemas.insert("resume".into(), target);
    Value::Object(schemas)
}

pub fn execute(app: &mut Application, command: &str, arguments: &Value) -> Result<Value> {
    let context = CommandContext::capture(app.clock(), "external", None);
    match command {
        "notify" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Notification {
                text: String,
            }
            let notification: Notification = serde_json::from_value(arguments.clone())?;
            app.notify(notification.text)?;
            Ok(serde_json::json!({"ok":true}))
        }
        "list" => execute_tool(app, "list_reminders", &arguments.to_string(), &context),
        "create" => execute_tool(app, "create_reminder", &arguments.to_string(), &context),
        "update" => execute_tool(app, "update_reminder", &arguments.to_string(), &context),
        "undo" => execute_tool(app, "undo_last_operation", &arguments.to_string(), &context),
        "delete" => execute_tool(app, "delete_reminder", &arguments.to_string(), &context),
        "pause" | "resume" => {
            let target: ReminderTarget = serde_json::from_value(arguments.clone())?;
            execute_tool(
                app,
                "set_reminder_paused",
                &json!({"id":target.id,"paused":command=="pause"}).to_string(),
                &context,
            )
        }
        _ => bail!("Unknown command: {command}"),
    }
}

/// Complete a command at the desktop adapter boundary without reversing saved facts.
pub fn with_desktop_result(
    command: &str,
    mut value: serde_json::Value,
    adapter: Result<()>,
) -> Result<serde_json::Value> {
    if let Err(error) = adapter {
        if command == "notify" {
            anyhow::bail!(
                "Notification creation could not be confirmed; a card may already exist: {error:#}"
            );
        }
        value["warnings"] = serde_json::json!([format!(
            "Command completed; desktop reconciliation failed: {error:#}"
        )]);
    }
    Ok(value)
}
