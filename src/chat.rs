use anyhow::{Context, Result};
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    types::{
        ChatCompletionRequestMessage, ChatCompletionResponseMessage, ChatCompletionTool,
        CreateChatCompletionRequest,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}
impl Default for Connection {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_for: Option<uuid::Uuid>,
}

pub struct ChatSession {
    directory: PathBuf,
    connection: Connection,
    messages: Vec<Message>,
    pending: Option<crate::commands::CommandRecovery>,
    redaction_keys: Vec<String>,
}
impl ChatSession {
    pub fn default_directory() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("win-notifi-toast")
    }
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory).context("Не удалось открыть папку настроек")?;
        fn read<T: serde::de::DeserializeOwned + Default>(path: PathBuf) -> Result<T> {
            match fs::read(path) {
                Ok(bytes) => Ok(serde_json::from_slice(&bytes)
                    .context("Повреждён файл настроек или истории")?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
                Err(e) => Err(e.into()),
            }
        }
        let connection: Connection = read(directory.join("connection.json"))?;
        let redaction_keys = vec![connection.api_key.clone()];
        Ok(Self {
            connection,
            redaction_keys,
            messages: read(directory.join("chat.json"))?,
            pending: read(directory.join("pending-command.json"))?,
            directory,
        })
    }
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
    pub fn pending(&self) -> Option<&crate::commands::CommandRecovery> {
        self.pending.as_ref()
    }
    pub fn save_pending(
        &mut self,
        pending: Option<crate::commands::CommandRecovery>,
    ) -> Result<()> {
        let mut value = serde_json::to_value(pending)?;
        for key in self.redaction_keys.iter().filter(|key| !key.is_empty()) {
            redact_chat_value(&mut value, key, None);
        }
        let pending = serde_json::from_value(value.clone())?;
        let prepared = self.directory.join("pending-command.pending");
        let mut file = fs::File::create(&prepared)?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(&value)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(prepared, self.directory.join("pending-command.json"))
            .context("Не удалось сохранить незавершённый ответ")?;
        self.pending = pending;
        Ok(())
    }
    pub fn save_connection(&mut self, connection: Connection) -> Result<()> {
        fs::write(
            self.directory.join("connection.json"),
            serde_json::to_vec_pretty(&connection)?,
        )
        .context("Не удалось сохранить подключение")?;
        if !self.redaction_keys.contains(&connection.api_key) {
            self.redaction_keys.push(connection.api_key.clone());
        }
        self.connection = connection;
        Ok(())
    }
    fn save_messages(&mut self, messages: Vec<Message>) -> Result<()> {
        fs::write(
            self.directory.join("chat.json"),
            serde_json::to_vec_pretty(&messages)?,
        )
        .context("Не удалось сохранить историю")?;
        self.messages = messages;
        Ok(())
    }
    pub fn record_user(&mut self, text: &str) -> Result<()> {
        self.record("user", text)
    }
    pub fn record_assistant(&mut self, text: &str) -> Result<()> {
        self.record("assistant", text)
    }
    /// Show committed operations immediately in one result card per command.
    pub fn record_action(&mut self, command_id: uuid::Uuid, text: &str) -> Result<()> {
        self.save_reply(Some(command_id), text, true)
    }
    /// The concluding reply replaces the progress card instead of duplicating it.
    pub fn finish_reply(&mut self, command_id: uuid::Uuid, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        let result_for = self
            .messages
            .iter()
            .any(|m| m.result_for == Some(command_id))
            .then_some(command_id);
        self.save_reply(result_for, text, false)
    }
    fn save_reply(
        &mut self,
        result_for: Option<uuid::Uuid>,
        text: &str,
        append: bool,
    ) -> Result<()> {
        let mut messages = self.messages.clone();
        let content = self.redact(text);
        if let Some(message) =
            result_for.and_then(|id| messages.iter_mut().find(|m| m.result_for == Some(id)))
        {
            if append {
                message.content.push('\n');
                message.content.push_str(&content);
            } else {
                message.content = content;
            }
        } else {
            messages.push(Message {
                role: "assistant".into(),
                content,
                result_for,
            });
        }
        self.save_messages(messages)
    }
    fn redact(&self, text: &str) -> String {
        self.redaction_keys
            .iter()
            .filter(|key| !key.is_empty())
            .fold(text.to_owned(), |text, key| {
                text.replace(key, "[ключ скрыт]")
            })
    }
    fn record(&mut self, role: &str, text: &str) -> Result<()> {
        let mut messages = self.messages.clone();
        let content = self.redact(text);
        messages.push(Message {
            role: role.into(),
            content,
            result_for: None,
        });
        self.save_messages(messages)
    }
    pub fn clear(&mut self) -> Result<()> {
        self.save_pending(None)?;
        self.save_messages(Vec::new())
    }
}
fn redact_chat_value(value: &mut serde_json::Value, key: &str, field: Option<&str>) {
    use serde_json::Value;
    match value {
        Value::String(text)
            if !matches!(
                field,
                Some(
                    "id" | "command_id"
                        | "call_id"
                        | "tool_call_id"
                        | "name"
                        | "role"
                        | "submitted_utc"
                        | "submitted_local"
                )
            ) =>
        {
            if let Ok(mut embedded) = serde_json::from_str::<Value>(text)
                && (embedded.is_object() || embedded.is_array())
            {
                redact_content(&mut embedded, key);
                *text = embedded.to_string();
            } else {
                *text = text.replace(key, "[ключ скрыт]");
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_chat_value(item, key, None);
            }
        }
        Value::Object(fields) => {
            for (name, value) in fields {
                redact_chat_value(value, key, Some(name));
            }
        }
        _ => {}
    }
}
fn redact_content(value: &mut serde_json::Value, key: &str) {
    use serde_json::Value;
    match value {
        Value::String(text) => *text = text.replace(key, "[ключ скрыт]"),
        Value::Array(items) => {
            for item in items {
                redact_content(item, key);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_content(value, key);
            }
        }
        _ => {}
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionError {
    Url,
    Authorization,
    Model,
    Network,
    Tools,
    Response,
}
impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
  Self::Url => "Проверьте базовый URL: нужен HTTP или HTTPS без ключа в адресе.",
  Self::Authorization => "Провайдер отклонил API-ключ. Проверьте ключ и доступ к модели.",
  Self::Model => "Модель не найдена или не поддерживает этот запрос. Проверьте ID и поддержку инструментов.",
  Self::Network => "Нет ответа от провайдера. Проверьте адрес, сеть и доступность сервиса.",
  Self::Tools => "Модель не выполнила проверочный вызов инструмента. Выберите модель с tool calling.",
  Self::Response => "Не удалось прочитать ответ провайдера. Проверьте совместимость Chat Completions.",
 })
    }
}
impl std::error::Error for ConnectionError {}
#[derive(Clone)]
pub struct Provider {
    client: Client<OpenAIConfig>,
    model: String,
}
impl Provider {
    pub fn new(connection: &Connection) -> std::result::Result<Self, ConnectionError> {
        let url =
            reqwest::Url::parse(connection.base_url.trim()).map_err(|_| ConnectionError::Url)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConnectionError::Url);
        }
        if connection.model.trim().is_empty() {
            return Err(ConnectionError::Model);
        }
        if connection.api_key.trim().is_empty() {
            return Err(ConnectionError::Authorization);
        }
        let config = OpenAIConfig::new()
            .with_api_base(connection.base_url.trim().trim_end_matches('/'))
            .with_api_key(connection.api_key.clone());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|_| ConnectionError::Network)?;
        Ok(Self {
            client: Client::with_config(config).with_http_client(http),
            model: connection.model.clone(),
        })
    }
    pub async fn complete(
        &self,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTool>,
        force_probe: bool,
    ) -> std::result::Result<ChatCompletionResponseMessage, ConnectionError> {
        let request: CreateChatCompletionRequest = serde_json::from_value(json!({"model":self.model,"messages":messages,"tools":tools,"tool_choice":if force_probe {json!({"type":"function","function":{"name":"connection_probe"}})} else {json!("auto")}})).map_err(|_| ConnectionError::Response)?;
        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| match e {
                OpenAIError::ApiError(e) => {
                    let code = format!(
                        "{} {}",
                        e.code.unwrap_or_default(),
                        e.r#type.unwrap_or_default()
                    )
                    .to_lowercase();
                    if code.contains("key") || code.contains("auth") || code.contains("permission")
                    {
                        ConnectionError::Authorization
                    } else {
                        ConnectionError::Model
                    }
                }
                OpenAIError::Reqwest(e)
                    if e.status()
                        .is_some_and(|s| s.as_u16() == 401 || s.as_u16() == 403) =>
                {
                    ConnectionError::Authorization
                }
                OpenAIError::Reqwest(_) => ConnectionError::Network,
                _ => ConnectionError::Response,
            })?;
        response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or(ConnectionError::Response)
    }
    pub async fn probe(&self) -> std::result::Result<(), ConnectionError> {
        let messages = serde_json::from_value(json!([{"role":"user","content":"Вызови connection_probe с аргументом ok=true. Это безопасная проверка подключения."}])).unwrap();
        let tool = serde_json::from_value(json!({"type":"function","function":{"name":"connection_probe","description":"Безопасная проверка инструментов, не изменяет данные.","parameters":{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}}})).unwrap();
        let response = self.complete(messages, vec![tool], true).await?;
        let valid = response.tool_calls.unwrap_or_default().iter().any(|t| {
            t.function.name == "connection_probe"
                && serde_json::from_str::<serde_json::Value>(&t.function.arguments)
                    .is_ok_and(|v| v == json!({"ok":true}))
        });
        if valid {
            Ok(())
        } else {
            Err(ConnectionError::Tools)
        }
    }
    pub async fn reply(&self, history: &[Message]) -> std::result::Result<String, ConnectionError> {
        let mut messages = vec![
            json!({"role":"system","content":"Отвечай по-русски. Не утверждай, что напоминание создано или изменено: инструменты напоминаний пока не подключены. Помогай уточнять запрос."}),
        ];
        messages.extend(
            history
                .iter()
                .map(|m| json!({"role":m.role,"content":m.content})),
        );
        // Probe tool is harmless and lets providers accept tools even before reminder tools are installed.
        let tools = vec![serde_json::from_value(json!({"type":"function","function":{"name":"connection_probe","description":"Проверка подключения без изменения данных","parameters":{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}}})).unwrap()];
        let response = self
            .complete(
                serde_json::from_value(json!(messages)).unwrap(),
                tools,
                false,
            )
            .await?;
        response
            .content
            .filter(|s| !s.trim().is_empty())
            .ok_or(ConnectionError::Response)
    }
}
