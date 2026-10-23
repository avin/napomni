//! Local commands use the same application and history as the internal chat.
use crate::{reminder_tools::execute_tool, reminders::{Application, CommandContext}};
use anyhow::{Result, bail};
use serde_json::Value;

pub fn execute(app: &mut Application, command: &str, arguments: &Value) -> Result<Value> {
    let context = CommandContext::capture(app.clock(), "external", None);
    match command {
        "list" => execute_tool(app, "list_reminders", &arguments.to_string(), &context),
        _ => bail!("Unknown command: {command}"),
    }
}
