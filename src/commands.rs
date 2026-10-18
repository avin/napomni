//! Durable tool receipts scoped to a trusted user command, independent of provider IDs.
use crate::{
    reminder_tools::execute_tool,
    reminders::{Application, CommandContext},
};
use anyhow::{Result, ensure};
use async_openai::types::ChatCompletionRequestMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandRecovery {
    pub context: CommandContext,
    pub messages: Vec<ChatCompletionRequestMessage>,
    pub facts: Vec<String>,
}
impl CommandRecovery {
    pub(crate) fn hydrate_receipts(&mut self, app: &Application) {
        let mut receipts = app
            .state
            .tool_receipts
            .values()
            .filter(|r| r.command_id == self.context.id)
            .collect::<Vec<_>>();
        receipts.sort_by_key(|r| r.order);
        for receipt in receipts {
            if !self.messages.iter().any(|message|matches!(message,ChatCompletionRequestMessage::Tool(tool) if tool.tool_call_id==receipt.call_id)) {
                self.messages.push(serde_json::from_value(json!({"role":"assistant","content":null,"tool_calls":[{"id":receipt.call_id,"type":"function","function":{"name":receipt.name,"arguments":receipt.arguments.to_string()}}]})).expect("saved tool call"));
                self.messages.push(serde_json::from_value(json!({"role":"tool","tool_call_id":receipt.call_id,"content":receipt.result.to_string()})).expect("saved tool result"));
            }
            if let Some(fact) = receipt.result.get("fact").and_then(Value::as_str)
                && !self.facts.iter().any(|existing| existing == fact)
            {
                self.facts.push(fact.into());
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolReceipt {
    pub command_id: uuid::Uuid,
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub result: Value,
    pub order: u64,
}
pub fn execute_tool_once(
    app: &mut Application,
    call_id: &str,
    name: &str,
    arguments: &str,
    context: &CommandContext,
) -> Result<Value> {
    ensure!(
        !call_id.trim().is_empty(),
        "У вызова инструмента отсутствует ID"
    );
    let parsed: Value = serde_json::from_str(arguments)
        .map_err(|_| anyhow::anyhow!("Неверный JSON аргументов инструмента"))?;
    let key = format!("{}:{call_id}", context.id);
    if let Some(receipt) = app.state.tool_receipts.get(&key) {
        ensure!(
            receipt.name == name && receipt.arguments == parsed,
            "Этот ID вызова уже выполнен с другими аргументами. Данные не изменены"
        );
        return Ok(receipt.result.clone());
    }
    app.transaction(|staged| {
        let result = execute_tool(staged, name, arguments, context)?;
        let receipt = ToolReceipt {
            command_id: context.id,
            call_id: call_id.into(),
            name: name.into(),
            arguments: parsed,
            result: result.clone(),
            order: u64::try_from(staged.state.tool_receipts.len())?,
        };
        staged.state.tool_receipts.insert(key, receipt);
        Ok(result)
    })
}
