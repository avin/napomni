//! Chat tool execution. Network awaits never hold the application lock.
use crate::{
    chat::{Message, Provider},
    commands::{CommandRecovery, execute_tool_once},
    reminders::{Application, CommandContext, CreateReminder},
};
use anyhow::{Result, bail};
use async_openai::types::ChatCompletionTool;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct CommandOutcome {
    pub text: String,
    pub facts: Vec<String>,
    pub error: Option<String>,
    pub recovery: Option<CommandRecovery>,
}
pub fn tools() -> Vec<ChatCompletionTool> {
    let mut tools:Vec<ChatCompletionTool>=serde_json::from_value(json!([
        {"type":"function","function":{"name":"list_reminders","description":"Read all saved reminders and their stable IDs.","parameters":{"type":"object","properties":{},"additionalProperties":false}}},
        {"type":"function","function":{"name":"create_reminder","description":"Save a one-time, interval, weekly, monthly, or yearly reminder. Call immediately when the schedule is determined; choose text and other defaults according to the system instructions. Clarify only material schedule ambiguities, including the starting week of an every-N-weeks cycle. Do not approximate unsupported rules.","parameters":{"type":"object","properties":{
            "text":{"type":"string","description":"Reminder text according to the system instructions. Copy quoted text verbatim."},
            "schedule":{"oneOf":[
                {"type":"object","properties":{"kind":{"const":"monthly"},"days":{"type":"array","minItems":1,"items":{"type":"integer","minimum":1,"maximum":31},"description":"Selected days of the month. Skip months without the requested day; do not shift dates."},"times":{"type":"array","minItems":1,"items":{"type":"string"},"description":"Windows local times in HH:MM:SS format"}},"required":["kind","days","times"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"yearly"},"dates":{"type":"array","minItems":1,"items":{"type":"object","properties":{"month":{"type":"integer","minimum":1,"maximum":12},"day":{"type":"integer","minimum":1,"maximum":31}},"required":["month","day"],"additionalProperties":false},"description":"Yearly dates. February 29 occurs only in leap years; do not shift invalid dates."},"times":{"type":"array","minItems":1,"items":{"type":"string"},"description":"Windows local times in HH:MM:SS format"}},"required":["kind","dates","times"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"interval"},"seconds":{"type":"integer","minimum":1,"description":"Positive elapsed-time interval in seconds"},"start_local":{"type":"string","description":"Optional explicit interval anchor in Windows local time, YYYY-MM-DDTHH:MM:SS. If omitted, the first occurrence is one interval after actual creation."}},"required":["kind","seconds"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"after_seconds"},"seconds":{"type":"integer","minimum":1}},"required":["kind","seconds"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"at_local"},"date_time":{"type":"string","description":"Windows local date and time: YYYY-MM-DDTHH:MM:SS"}},"required":["kind","date_time"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"tomorrow"},"time":{"type":"string","description":"Local time HH:MM:SS; the application computes tomorrow relative to command submission"}},"required":["kind","time"],"additionalProperties":false},
                {"type":"object","properties":{
                    "kind":{"const":"weekly"},
                    "weekdays":{"type":"array","items":{"type":"integer","minimum":1,"maximum":7},"minItems":1,"description":"Weekdays: 1 is Monday, 7 is Sunday; working days are 1-5"},
                    "times":{"type":"array","items":{"type":"string"},"minItems":1,"description":"Windows local times in HH:MM:SS format"},
                    "every_n_weeks":{"type":"integer","minimum":1,"description":"Positive number of weeks per cycle"},
                    "anchor_monday":{"type":"string","description":"YYYY-MM-DD: Monday of the first active week. Required for cycles >1; if the week is unspecified, ask first rather than inventing the cycle phase"}
                },"required":["kind","weekdays","times","every_n_weeks"],"additionalProperties":false}
            ]},
            "border":{"type":"integer","minimum":0,"maximum":16777215,"description":"RGB color. Defaults to a blue border."},
            "sound":{"type":"boolean","description":"Sound is enabled by default."}
        },"required":["text","schedule"],"additionalProperties":false}}}
    ])).expect("static tool definitions");
    let schedule = &mut tools[1]
        .function
        .parameters
        .as_mut()
        .expect("create parameters")["properties"]["schedule"];
    let repeating = schedule["oneOf"]
        .as_array()
        .expect("schedule variants")
        .iter()
        .filter(|variant| {
            matches!(
                variant["properties"]["kind"]["const"].as_str(),
                Some("interval" | "weekly" | "monthly" | "yearly")
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    schedule["oneOf"].as_array_mut().expect("schedule variants").push(json!({"type":"object","properties":{"kind":{"const":"constrained"},"base":{"oneOf":repeating},"limits":crate::constraints::schema()},"required":["kind","base","limits"],"additionalProperties":false}));
    let schedule = tools[1]
        .function
        .parameters
        .as_ref()
        .expect("create parameters")["properties"]["schedule"]
        .clone();
    tools.extend(crate::mutation_tools::tools(schedule));
    tools
}
pub fn execute_tool(
    app: &mut Application,
    name: &str,
    arguments: &str,
    context: &CommandContext,
) -> Result<Value> {
    match name {
        "list_reminders" => {
            let args: Value = serde_json::from_str(arguments)?;
            if args != json!({}) {
                bail!("Инструмент списка не принимает аргументы");
            }
            Ok(
                json!({"ok":true,"reminders":app.reminders().iter().map(|r|json!({"reminder":r,"schedule_description":r.schedule.description(app.clock())})).collect::<Vec<_>>() }),
            )
        }
        "create_reminder" => {
            let input: CreateReminder = serde_json::from_str(arguments).map_err(|_| {
                anyhow::anyhow!(
                    "Неверные аргументы напоминания. Уточните текст и поддерживаемое время"
                )
            })?;
            let reminder = app.create(input, context)?;
            let description = reminder.schedule.description(app.clock());
            Ok(
                json!({"ok":true,"reminder":reminder,"schedule_description":description,"fact":format!("Сохранено #{}: «{}». {}",reminder.id,reminder.text,description)}),
            )
        }
        _ => crate::mutation_tools::execute(app, name, arguments, context),
    }
}
pub async fn run_command(
    provider: &Provider,
    history: &[Message],
    context: CommandContext,
    app: Arc<Mutex<Application>>,
) -> CommandOutcome {
    run_command_with_progress(provider, history, context, app, |_| {}).await
}
/// Publishes committed facts before waiting for the model's concluding text.
pub async fn run_command_with_progress(
    provider: &Provider,
    history: &[Message],
    context: CommandContext,
    app: Arc<Mutex<Application>>,
    committed: impl FnMut(&str),
) -> CommandOutcome {
    match prepare_command(history, context) {
        Ok(recovery) => resume_command_with_progress(provider, recovery, app, committed).await,
        Err(error) => CommandOutcome {
            error: Some(error.to_string()),
            ..Default::default()
        },
    }
}
pub fn prepare_command(history: &[Message], context: CommandContext) -> Result<CommandRecovery> {
    let mut messages = vec![json!({"role":"system","content":format!(
        "You are a Windows reminder assistant. Reply in the language of the latest user message, unless the user explicitly requests another language. Earlier messages, tool results, and these English instructions do not determine the reply language. Use the same language for reminder text you generate; preserve user-provided quoted text verbatim. Only tools modify data; ordinary replies do not save anything. Report the actual tool result in the reply language, preserving its facts. When the user requests a reminder and its schedule can be determined unambiguously from the request and context, call create_reminder immediately without prior confirmation. Use the supplied wording for the text or briefly express the topic; if neither topic nor text is supplied, or the user allows any text, use the localized equivalent of 'Reminder'. Use tool defaults for unspecified sound and border color. For example, for 'I need a notification in 5 minutes with sound', immediately call create_reminder with text 'Reminder', schedule kind after_seconds, seconds 300, and sound true. Ask a clarification only when the answer is necessary to determine the schedule or identify an existing reminder unambiguously. Ask only for missing essential information and retain the supplied parameters and context. Do not invent facts or essential schedule parameters. Supported schedules include one-time, interval, and weekly reminders: selected weekdays (Monday=1, Sunday=7), multiple times, and every N weeks. For N>1, obtain the starting week and pass its Monday date as anchor_monday; ask if it is missing. Do not invent the cycle phase. Pass monthly recurrences as monthly with days and times, and yearly recurrences as yearly with dates (month, day) and times. Skip days absent from a month and February 29 in non-leap years without rescheduling. Do not save an impossible specific date as a different date. Calendar times follow Windows local time; working days mean Monday-Friday. Do not approximate unsupported rules. Pass date ranges, time windows, weekdays, and recurrence exclusions as constrained with an unchanged base rule and limits. Date and time bounds are inclusive; overnight windows and nested constraints are unsupported. Do not reschedule excluded dates or change the recurrence phase. When only constraints change, pass patch.limits and preserve unspecified constraints; empty limits removes all constraints without changing the base recurrence. When editing, pass ONLY explicitly requested patch properties. Changes require the ID selected by the user; identical text in multiple reminders does not authorize choosing arbitrarily. Without a selected ID, read the list and clarify any ambiguity. Use the corresponding tools to pause, delete, or undo. Pass relative delays as after_seconds and tomorrow as tomorrow: the application computes them from command submission. Pass every-N-seconds recurrences as interval with seconds; supply start_local only for an explicitly specified start, otherwise the application anchors the interval to actual creation. Command submission UTC: {}, Windows local time: {}. Selected ID: {:?}.",
        context.submitted_utc,context.submitted_local,context.selected_id
    )})];
    messages.extend(
        history
            .iter()
            .map(|m| json!({"role":m.role,"content":m.content})),
    );
    Ok(CommandRecovery {
        context,
        messages: serde_json::from_value(json!(messages))?,
        facts: vec![],
    })
}
pub async fn resume_command(
    provider: &Provider,
    recovery: CommandRecovery,
    app: Arc<Mutex<Application>>,
) -> CommandOutcome {
    resume_command_with_progress(provider, recovery, app, |_| {}).await
}
pub async fn resume_command_with_progress(
    provider: &Provider,
    mut recovery: CommandRecovery,
    app: Arc<Mutex<Application>>,
    mut committed: impl FnMut(&str),
) -> CommandOutcome {
    let original_facts = recovery.facts.len();
    if let Ok(app) = app.lock() {
        recovery.hydrate_receipts(&app);
    }
    for fact in recovery.facts.iter().skip(original_facts) {
        committed(fact);
    }
    let mut outcome = CommandOutcome {
        facts: recovery.facts.clone(),
        ..Default::default()
    };
    for _ in 0..8 {
        let response = match provider
            .complete(recovery.messages.clone(), tools(), false)
            .await
        {
            Ok(r) => r,
            Err(error) => {
                outcome.error = Some(error.to_string());
                break;
            }
        };
        let calls = response.tool_calls.unwrap_or_default();
        if calls.is_empty() {
            outcome.text = response.content.unwrap_or_default();
            return outcome;
        }
        recovery.messages.push(
            serde_json::from_value(
                json!({"role":"assistant","content":response.content,"tool_calls":calls}),
            )
            .expect("typed assistant tools"),
        );
        for call in calls {
            let result = match app.lock() {
                Ok(mut app) => execute_tool_once(
                    &mut app,
                    &call.id,
                    &call.function.name,
                    &call.function.arguments,
                    &recovery.context,
                ),
                Err(_) => Err(anyhow::anyhow!("Прикладное ядро недоступно")),
            };
            let value = match result {
                Ok(value) => {
                    if let Some(fact) = value.get("fact").and_then(Value::as_str)
                        && !outcome.facts.iter().any(|existing| existing == fact)
                    {
                        committed(fact);
                        outcome.facts.push(fact.into());
                    }
                    value
                }
                Err(error) => json!({"ok":false,"error":error.to_string()}),
            };
            recovery.messages.push(
                serde_json::from_value(
                    json!({"role":"tool","tool_call_id":call.id,"content":value.to_string()}),
                )
                .expect("typed tool result"),
            );
        }
    }
    if outcome.error.is_none() {
        outcome.error = Some("Обработка остановлена: слишком много вызовов инструментов".into());
    }
    recovery.facts = outcome.facts.clone();
    outcome.recovery = Some(recovery);
    outcome
}
