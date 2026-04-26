use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use crate::models::{ImportedChat, Provider};

use super::{JsonFile, estimate_tokens, snippet, stable_chat_hash};

pub(super) fn parse(files: &[JsonFile], source_hash: &str) -> Result<Vec<ImportedChat>> {
    let mut chats = Vec::new();
    for file in files {
        if !file
            .name
            .to_ascii_lowercase()
            .ends_with("conversations.json")
        {
            continue;
        }
        if let Some(items) = file.value.as_array() {
            for item in items {
                if let Some(chat) = parse_conversation(item, source_hash) {
                    chats.push(chat);
                }
            }
        }
    }
    Ok(chats)
}

fn parse_conversation(value: &Value, source_hash: &str) -> Option<ImportedChat> {
    let title = string_field(value, &["title"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled ChatGPT chat".to_string());
    let created_at = unix_field(value, "create_time");
    let updated_at = unix_field(value, "update_time");
    let id = string_field(value, &["id"]).unwrap_or_default();

    let mut input = 0;
    let mut output = 0;
    let mut first_snippet = None;
    let mut model = string_field(value, &["model", "model_slug"]);

    if let Some(mapping) = value.get("mapping").and_then(Value::as_object) {
        for node in mapping.values() {
            let Some(message) = node.get("message") else {
                continue;
            };
            let role = message
                .pointer("/author/role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if model.is_none() {
                model = model_hint(message);
            }
            let text = message_text(message);
            if text.trim().is_empty() {
                continue;
            }
            if first_snippet.is_none() {
                first_snippet = snippet(&text);
            }
            match role {
                "user" | "system" | "tool" => input += estimate_tokens(&text),
                "assistant" => output += estimate_tokens(&text),
                _ => {}
            }
        }
    }

    let source_hash = stable_chat_hash(
        source_hash,
        "chatgpt",
        &[
            id,
            title.clone(),
            created_at.map(|v| v.to_rfc3339()).unwrap_or_default(),
        ],
    );

    Some(ImportedChat {
        provider: Provider::OpenAi,
        title,
        created_at,
        updated_at,
        model,
        estimated_input_tokens: input,
        estimated_output_tokens: output,
        estimated_cost_usd: None,
        source_hash,
        snippet: first_snippet,
    })
}

fn model_hint(value: &Value) -> Option<String> {
    value
        .pointer("/metadata/model_slug")
        .or_else(|| value.pointer("/metadata/default_model_slug"))
        .or_else(|| value.pointer("/metadata/model"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn message_text(message: &Value) -> String {
    let Some(content) = message.get("content") else {
        return String::new();
    };
    if let Some(parts) = content.get("parts").and_then(Value::as_array) {
        return parts
            .iter()
            .filter_map(|part| {
                if let Some(text) = part.as_str() {
                    Some(text.to_string())
                } else {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn unix_field(value: &Value, name: &str) -> Option<DateTime<Utc>> {
    let seconds = value.get(name).and_then(Value::as_f64)?;
    Utc.timestamp_opt(seconds as i64, 0).single()
}
