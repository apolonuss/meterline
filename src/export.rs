use anyhow::{Context, Result};
use serde::Serialize;
use std::io::Write;
use std::path::Path;

use crate::models::{CostBucket, Dashboard, ImportedChat, UsageBucket};
use crate::store::Store;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Serialize)]
struct ExportBundle {
    dashboard: Dashboard,
    usage: Vec<UsageBucket>,
    costs: Vec<CostBucket>,
    chats: Vec<ImportedChat>,
}

#[derive(Serialize)]
struct CsvRow {
    kind: String,
    provider: String,
    model: String,
    start_time: String,
    end_time: String,
    title: String,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    requests: i64,
    amount: f64,
    currency: String,
    line_item: String,
    source_hash: String,
}

pub fn export_store(store: &Store, format: ExportFormat, output: Option<&Path>) -> Result<()> {
    let bundle = ExportBundle {
        dashboard: store.dashboard()?,
        usage: store.usage_buckets()?,
        costs: store.cost_buckets()?,
        chats: store.imported_chats()?,
    };

    let mut bytes = Vec::new();
    match format {
        ExportFormat::Json => serde_json::to_writer_pretty(&mut bytes, &bundle)?,
        ExportFormat::Csv => write_csv(&mut bytes, &bundle)?,
    }

    if let Some(output) = output {
        std::fs::write(output, bytes)
            .with_context(|| format!("could not write {}", output.display()))?;
    } else {
        std::io::stdout().write_all(&bytes)?;
        std::io::stdout().write_all(b"\n")?;
    }

    Ok(())
}

fn write_csv(writer: impl Write, bundle: &ExportBundle) -> Result<()> {
    let mut csv = csv::Writer::from_writer(writer);
    for usage in &bundle.usage {
        csv.serialize(CsvRow {
            kind: "usage".to_string(),
            provider: usage.provider.as_str().to_string(),
            model: usage.model.clone().unwrap_or_default(),
            start_time: usage.start_time.to_rfc3339(),
            end_time: usage.end_time.to_rfc3339(),
            title: String::new(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            requests: usage.requests,
            amount: 0.0,
            currency: String::new(),
            line_item: String::new(),
            source_hash: String::new(),
        })?;
    }
    for cost in &bundle.costs {
        csv.serialize(CsvRow {
            kind: "cost".to_string(),
            provider: cost.provider.as_str().to_string(),
            model: cost.model.clone().unwrap_or_default(),
            start_time: cost.start_time.to_rfc3339(),
            end_time: cost.end_time.to_rfc3339(),
            title: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            requests: 0,
            amount: cost.amount,
            currency: cost.currency.clone(),
            line_item: cost.line_item.clone().unwrap_or_default(),
            source_hash: String::new(),
        })?;
    }
    for chat in &bundle.chats {
        csv.serialize(CsvRow {
            kind: "chat".to_string(),
            provider: chat.provider.as_str().to_string(),
            model: chat.model.clone().unwrap_or_default(),
            start_time: chat
                .created_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            end_time: chat
                .updated_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            title: chat.title.clone(),
            input_tokens: chat.estimated_input_tokens,
            output_tokens: chat.estimated_output_tokens,
            cached_input_tokens: 0,
            requests: 0,
            amount: chat.estimated_cost_usd.unwrap_or_default(),
            currency: "usd".to_string(),
            line_item: String::new(),
            source_hash: chat.source_hash.clone(),
        })?;
    }
    csv.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ImportProvider, ImportedChat, Provider};
    use crate::store::Store;
    use tempfile::tempdir;

    #[test]
    fn exports_json_and_csv() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("meterline.sqlite3"), "test-key").unwrap();
        store
            .insert_imported_chats(
                ImportProvider::ChatGpt,
                "source.zip",
                "sourcehash",
                &[ImportedChat {
                    provider: Provider::OpenAi,
                    title: "Export me".to_string(),
                    created_at: None,
                    updated_at: None,
                    model: Some("gpt-test".to_string()),
                    estimated_input_tokens: 1,
                    estimated_output_tokens: 2,
                    estimated_cost_usd: None,
                    source_hash: "chat-hash".to_string(),
                    snippet: None,
                }],
            )
            .unwrap();

        let json_path = dir.path().join("meterline.json");
        let csv_path = dir.path().join("meterline.csv");
        export_store(&store, ExportFormat::Json, Some(&json_path)).unwrap();
        export_store(&store, ExportFormat::Csv, Some(&csv_path)).unwrap();
        assert!(
            std::fs::read_to_string(json_path)
                .unwrap()
                .contains("Export me")
        );
        assert!(std::fs::read_to_string(csv_path).unwrap().contains("chat"));
    }
}
