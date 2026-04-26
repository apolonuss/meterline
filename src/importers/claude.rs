use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::models::{ImportedChat, Provider};

use super::{JsonFile, estimate_tokens, snippet, stable_chat_hash};

pub(super) fn parse(files: &[JsonFile], source_hash: &str) -> Result<Vec<ImportedChat>> {
    let mut chats = Vec::new();
    for file in files {
        if let Some(items) = file.value.as_array() {
            for item in items {
                if looks_like_conversation(item) {
                    if let Some(chat) = parse_conversation(item, source_hash) {
                        chats.push(chat);
                    }
                }
            }
        } else if looks_like_conversation(&file.value) {
            if let Some(chat) = parse_conversation(&file.value, source_hash) {
                chats.push(chat);
            }
        }
    }
    Ok(chats)
}

fn looks_like_conversation(value: &Value) -> bool {
    value.get("chat_messages").is_some()
        || value.get("messages").is_some()
        || value.get("name").is_some() && value.get("uuid").is_some()
}

fn parse_conversation(value: &Value, source_hash: &str) -> Option<ImportedChat> {
    let title = string_field(value, &["name", "title"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled Claude chat".to_string());
    let created_at = time_field(value, &["created_at", "createdAt"]);
    let updated_at = time_field(value, &["updated_at", "updatedAt"]);
    let id = string_field(value, &["uuid", "id"]).unwrap_or_default();
    let model = string_field(value, &["model", "model_slug"]);

    let mut input = 0;
    let mut output = 0;
    let mut first_snippet = None;
    let messages = value
        .get("chat_messages")
        .or_else(|| value.get("messages"))
        .and_then(Value::as_array);

    if let Some(messages) = messages {
        for message in messages {
            let sender = string_field(message, &["sender", "role"]).unwrap_or_default();
            let text = message_text(message);
            if text.trim().is_empty() {
                continue;
            }
            if first_snippet.is_none() {
                first_snippet = snippet(&text);
            }
            match sender.as_str() {
                "human" | "user" | "system" => input += estimate_tokens(&text),
                "assistant" | "model" | "claude" => output += estimate_tokens(&text),
                _ => {}
            }
        }
    }

    let source_hash = stable_chat_hash(
        source_hash,
        "claude",
        &[
            id,
            title.clone(),
            created_at.map(|v| v.to_rfc3339()).unwrap_or_default(),
        ],
    );

    Some(ImportedChat {
        provider: Provider::Claude,
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

fn message_text(message: &Value) -> String {
    if let Some(text) = string_field(message, &["text", "content"]) {
        return text;
    }
    if let Some(content) = message.get("content").and_then(Value::as_array) {
        return content
            .iter()
            .filter_map(|part| {
                part.as_str().map(ToOwned::to_owned).or_else(|| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn time_field(value: &Value, names: &[&str]) -> Option<DateTime<Utc>> {
    let text = names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))?;
    DateTime::parse_from_rfc3339(text)
        .map(|value| value.with_timezone(&Utc))
        .ok()
}
