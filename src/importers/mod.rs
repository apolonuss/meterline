mod chatgpt;
mod claude;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

use crate::models::{ImportProvider, ImportedChat};

#[derive(Debug)]
pub struct ImportArchive {
    pub source_hash: String,
    pub chats: Vec<ImportedChat>,
}

pub fn import_archive(provider: ImportProvider, path: &Path) -> Result<ImportArchive> {
    let bytes =
        std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let source_hash = hex::encode(Sha256::digest(&bytes));
    let files = read_json_files(&bytes)?;
    let chats = match provider {
        ImportProvider::ChatGpt => chatgpt::parse(&files, &source_hash)?,
        ImportProvider::Claude => claude::parse(&files, &source_hash)?,
    };

    if chats.is_empty() {
        bail!("no conversations were found in {}", path.display());
    }

    Ok(ImportArchive { source_hash, chats })
}

#[derive(Debug)]
struct JsonFile {
    name: String,
    value: serde_json::Value,
}

fn read_json_files(bytes: &[u8]) -> Result<Vec<JsonFile>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("not a valid zip archive")?;
    let mut files = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if !file.name().to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .with_context(|| format!("could not read {}", file.name()))?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            files.push(JsonFile {
                name: file.name().to_string(),
                value,
            });
        }
    }

    Ok(files)
}

fn estimate_tokens(text: &str) -> i64 {
    let chars = text.chars().filter(|value| !value.is_control()).count() as i64;
    (chars / 4).max(if chars > 0 { 1 } else { 0 })
}

fn snippet(text: &str) -> Option<String> {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        None
    } else {
        Some(compact.chars().take(180).collect())
    }
}

fn stable_chat_hash(source_hash: &str, provider: &str, parts: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_hash.as_bytes());
    hasher.update(provider.as_bytes());
    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn imports_chatgpt_zip_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chatgpt.zip");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("conversations.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"[{
                "id": "conv-1",
                "title": "Usage idea",
                "create_time": 1700000000.0,
                "update_time": 1700000100.0,
                "mapping": {
                    "a": {"message": {"author": {"role": "user"}, "content": {"parts": ["Hello model"]}}},
                    "b": {"message": {"author": {"role": "assistant"}, "metadata": {"model_slug": "gpt-4o-mini"}, "content": {"parts": ["Hi human"]}}}
                }
            }]"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let archive = import_archive(ImportProvider::ChatGpt, &path).unwrap();
        assert_eq!(archive.chats.len(), 1);
        assert_eq!(archive.chats[0].title, "Usage idea");
        assert_eq!(archive.chats[0].model.as_deref(), Some("gpt-4o-mini"));
        assert!(archive.chats[0].estimated_input_tokens > 0);
    }

    #[test]
    fn imports_claude_zip_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("claude.zip");
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("conversations.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"[{
                "uuid": "conv-1",
                "name": "Budget",
                "created_at": "2025-01-01T00:00:00Z",
                "updated_at": "2025-01-01T00:01:00Z",
                "chat_messages": [
                    {"sender": "human", "text": "Track this"},
                    {"sender": "assistant", "text": "Tracked"}
                ]
            }]"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let archive = import_archive(ImportProvider::Claude, &path).unwrap();
        assert_eq!(archive.chats.len(), 1);
        assert_eq!(archive.chats[0].title, "Budget");
        assert!(archive.chats[0].estimated_output_tokens > 0);
    }
}
