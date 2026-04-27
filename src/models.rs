use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[value(name = "openai", alias = "chatgpt")]
    OpenAi,
    #[value(name = "claude", alias = "anthropic")]
    Claude,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::Claude => "claude",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Claude => "Claude",
        }
    }
}

impl Display for Provider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "openai" | "chatgpt" => Ok(Provider::OpenAi),
            "claude" | "anthropic" => Ok(Provider::Claude),
            _ => Err(format!("unsupported provider: {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportProvider {
    #[value(name = "chatgpt", alias = "openai")]
    ChatGpt,
    #[value(name = "claude", alias = "anthropic")]
    Claude,
}

impl ImportProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportProvider::ChatGpt => "chatgpt",
            ImportProvider::Claude => "claude",
        }
    }

    pub fn provider(self) -> Provider {
        match self {
            ImportProvider::ChatGpt => Provider::OpenAi,
            ImportProvider::Claude => Provider::Claude,
        }
    }
}

impl Display for ImportProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ImportProvider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "chatgpt" | "openai" => Ok(ImportProvider::ChatGpt),
            "claude" | "anthropic" => Ok(ImportProvider::Claude),
            _ => Err(format!("unsupported import provider: {value}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub provider: Provider,
    pub label: String,
    pub connected_at: DateTime<Utc>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageBucket {
    pub provider: Provider,
    pub model: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub requests: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CostBucket {
    pub provider: Provider,
    pub model: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub amount: f64,
    pub currency: String,
    pub line_item: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedChat {
    pub provider: Provider,
    pub title: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub estimated_input_tokens: i64,
    pub estimated_output_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub source_hash: String,
    pub snippet: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveRequest {
    pub provider: Provider,
    pub method: String,
    pub path: String,
    pub model: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub status_code: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub request_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportRun {
    pub provider: ImportProvider,
    pub source_path: String,
    pub source_hash: String,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub ran_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ModelSummary {
    pub provider: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub requests: i64,
    pub cost_usd: f64,
    pub imported_chats: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HourlyUsageSummary {
    pub provider: String,
    pub hour_utc: u8,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub requests: i64,
    pub imported_chats: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Dashboard {
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_requests: i64,
    #[serde(default)]
    pub live_request_count: i64,
    pub imported_chats: i64,
    pub providers: Vec<ProviderAccount>,
    pub models: Vec<ModelSummary>,
    pub recent_chats: Vec<ImportedChat>,
    #[serde(default)]
    pub recent_live_requests: Vec<LiveRequest>,
    pub import_runs: Vec<ImportRun>,
    #[serde(default)]
    pub hourly_usage: Vec<HourlyUsageSummary>,
}
