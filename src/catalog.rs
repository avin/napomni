use crate::chat::{ChatSession, Connection};
use anyhow::{Result, bail};
use async_openai::{Client, config::OpenAIConfig};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSupport {
    Supported,
    Unknown,
    Unsupported,
}
#[derive(Clone, Debug)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub tool_support: ToolSupport,
}
#[derive(Clone, Default)]
pub struct Catalog {
    models: Vec<CatalogModel>,
}
#[derive(Deserialize)]
struct CatalogResponse {
    data: Vec<ModelResponse>,
}
#[derive(Deserialize)]
struct ModelResponse {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}
impl Catalog {
    pub async fn load(connection: &Connection) -> Result<Self> {
        let url = reqwest::Url::parse(connection.base_url.trim())
            .map_err(|_| anyhow::anyhow!("Проверьте базовый URL каталога"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("Проверьте базовый URL каталога");
        }
        let config = OpenAIConfig::new()
            .with_api_base(connection.base_url.trim().trim_end_matches('/'))
            .with_api_key(connection.api_key.clone());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let client = Client::with_config(config).with_http_client(http);
        let response: CatalogResponse = client.models().list_byot().await.map_err(|_| {
            anyhow::anyhow!(
                "Каталог недоступен. Проверьте адрес, ключ и сеть; ID модели можно ввести вручную."
            )
        })?;
        Ok(Self {
            models: response
                .data
                .into_iter()
                .filter(|m| !m.id.trim().is_empty())
                .map(|m| CatalogModel {
                    name: m
                        .name
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| m.id.clone()),
                    id: m.id,
                    tool_support: match m.supported_parameters {
                        Some(parameters) if parameters.iter().any(|p| p == "tools") => {
                            ToolSupport::Supported
                        }
                        Some(_) => ToolSupport::Unsupported,
                        None => ToolSupport::Unknown,
                    },
                })
                .collect(),
        })
    }
    pub fn search(&self, query: &str) -> Vec<&CatalogModel> {
        let query = query.trim().to_lowercase();
        self.models
            .iter()
            .filter(|m| {
                m.tool_support != ToolSupport::Unsupported
                    && (m.name.to_lowercase().contains(&query)
                        || m.id.to_lowercase().contains(&query))
            })
            .collect()
    }
    pub fn select(&self, id: &str, chat: &mut ChatSession) -> Result<()> {
        if !self
            .models
            .iter()
            .any(|m| m.id == id && m.tool_support != ToolSupport::Unsupported)
        {
            bail!("Эта модель недоступна для выбора в каталоге");
        }
        let mut connection = chat.connection().clone();
        connection.model = id.to_owned();
        chat.save_connection(connection)
    }
}
