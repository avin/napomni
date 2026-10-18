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
        {"type":"function","function":{"name":"list_reminders","description":"Прочитать все сохранённые напоминания и их устойчивые ID.","parameters":{"type":"object","properties":{},"additionalProperties":false}}},
        {"type":"function","function":{"name":"create_reminder","description":"Сохранить однократное, интервальное, недельное, ежемесячное или ежегодное напоминание. До вызова уточни неоднозначное время, текст и начальную неделю цикла каждые N недель. Неподдерживаемые правила не заменяй приблизительными.","parameters":{"type":"object","properties":{
            "text":{"type":"string","description":"Текст напоминания. Текст в кавычках копируй дословно."},
            "schedule":{"oneOf":[
                {"type":"object","properties":{"kind":{"const":"monthly"},"days":{"type":"array","minItems":1,"items":{"type":"integer","minimum":1,"maximum":31},"description":"Выбранные числа месяца. Месяцы без нужного числа пропускаются без переноса."},"times":{"type":"array","minItems":1,"items":{"type":"string"},"description":"Местные времена Windows HH:MM:SS"}},"required":["kind","days","times"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"yearly"},"dates":{"type":"array","minItems":1,"items":{"type":"object","properties":{"month":{"type":"integer","minimum":1,"maximum":12},"day":{"type":"integer","minimum":1,"maximum":31}},"required":["month","day"],"additionalProperties":false},"description":"Ежегодные даты. 29 февраля только в високосный год; невозможные даты не переносить."},"times":{"type":"array","minItems":1,"items":{"type":"string"},"description":"Местные времена Windows HH:MM:SS"}},"required":["kind","dates","times"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"interval"},"seconds":{"type":"integer","minimum":1,"description":"Положительный реальный интервал в секундах"},"start_local":{"type":"string","description":"Необязательное явное начало сетки в местном времени Windows YYYY-MM-DDTHH:MM:SS. Без него первое срабатывание через интервал после фактического создания."}},"required":["kind","seconds"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"after_seconds"},"seconds":{"type":"integer","minimum":1}},"required":["kind","seconds"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"at_local"},"date_time":{"type":"string","description":"Местная дата и время Windows: YYYY-MM-DDTHH:MM:SS"}},"required":["kind","date_time"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"tomorrow"},"time":{"type":"string","description":"Местное время HH:MM:SS; завтра вычисляет приложение от отправки команды"}},"required":["kind","time"],"additionalProperties":false},
                {"type":"object","properties":{
                    "kind":{"const":"weekly"},
                    "weekdays":{"type":"array","items":{"type":"integer","minimum":1,"maximum":7},"minItems":1,"description":"Дни недели: 1 понедельник, 7 воскресенье; будни 1–5"},
                    "times":{"type":"array","items":{"type":"string"},"minItems":1,"description":"Местные времена Windows HH:MM:SS"},
                    "every_n_weeks":{"type":"integer","minimum":1,"description":"Положительное число недель в цикле"},
                    "anchor_monday":{"type":"string","description":"YYYY-MM-DD: понедельник первой активной недели. Обязательно для цикла >1; если неделя не названа, сначала уточни, не придумывай фазу"}
                },"required":["kind","weekdays","times","every_n_weeks"],"additionalProperties":false}
            ]},
            "border":{"type":"integer","minimum":0,"maximum":16777215,"description":"Цвет RGB. Без указания синяя рамка."},
            "sound":{"type":"boolean","description":"Без указания звук включён."}
        },"required":["text","schedule"],"additionalProperties":false}}}
    ])).expect("static tool definitions");
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
        "Ты помощник напоминаний Windows. Отвечай по-русски. Данные изменяют только инструменты; обычный текст ничего не сохраняет. Сообщай фактический результат инструмента. Если неясны время, дата, текст или нужная запись, сначала задай уточнение. Не выдумывай данные. Поддерживаются однократные, интервальные напоминания и недельные повторы: выбранные дни (понедельник=1, воскресенье=7), несколько времён, каждые N недель. Для N>1 обязательно уточни начальную неделю и передай дату её понедельника в anchor_monday. Не придумывай фазу цикла. Месячные повторы передавай как monthly с days и times; ежегодные как yearly с dates (month, day) и times. Числа, отсутствующие в месяце, и 29 февраля невисокосного года пропускаются без переноса. Невозможную конкретную дату не сохраняй под другим числом. Календарные времена следуют местным часам Windows; будни означают понедельник–пятницу. Неподдерживаемые правила не заменяй приблизительными. При редактировании передавай ТОЛЬКО явно названные свойства patch. Выбранный пользователем ID обязателен для изменения; одинаковые тексты разных записей не дают права выбрать произвольную. Без выбранного ID прочитай список и уточни неоднозначность. Для паузы, удаления и отмены используй соответствующие инструменты. Текст в кавычках сохраняй дословно. Относительное время передавай как after_seconds, слово завтра как tomorrow: приложение вычисляет их от отправки команды. Повтор каждые N секунд передавай как interval с seconds; start_local указывай только при явно заданном начале, без него сетку привязывает приложение к фактическому созданию. Отправка команды UTC: {}, местное время Windows: {}. Выбранный ID: {:?}.",
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
                    if let Some(fact) = value.get("fact").and_then(Value::as_str) {
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
