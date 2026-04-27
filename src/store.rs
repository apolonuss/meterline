#[cfg(feature = "encrypted-storage")]
use anyhow::bail;
use anyhow::{Context, Result};
use chrono::{DateTime, Timelike, Utc};
#[cfg(feature = "encrypted-storage")]
use rusqlite::OptionalExtension;
use rusqlite::types::Type;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;
use std::path::Path;

use crate::models::{
    CostBucket, Dashboard, HourlyUsageSummary, ImportProvider, ImportRun, ImportedChat,
    ModelSummary, Provider, ProviderAccount, UsageBucket,
};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path, key: &str) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("could not open database {}", path.display()))?;

        #[cfg(feature = "encrypted-storage")]
        {
            conn.pragma_update(None, "key", key)
                .context("could not set SQLCipher database key")?;

            let cipher_version = conn
                .query_row("PRAGMA cipher_version;", [], |row| row.get::<_, String>(0))
                .optional()
                .context("could not verify SQLCipher support")?;
            if cipher_version.unwrap_or_default().is_empty() {
                bail!(
                    "Meterline was built without SQLCipher support; encrypted SQLite is required"
                );
            }
        }

        #[cfg(not(feature = "encrypted-storage"))]
        {
            let _ = key;
        }

        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS provider_accounts (
                provider TEXT PRIMARY KEY NOT NULL,
                label TEXT NOT NULL,
                connected_at TEXT NOT NULL,
                last_synced_at TEXT
            );
            CREATE TABLE IF NOT EXISTS usage_buckets (
                provider TEXT NOT NULL,
                model TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cached_input_tokens INTEGER NOT NULL,
                requests INTEGER NOT NULL,
                PRIMARY KEY (provider, model, start_time, end_time)
            );
            CREATE TABLE IF NOT EXISTS cost_buckets (
                provider TEXT NOT NULL,
                model TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                amount REAL NOT NULL,
                currency TEXT NOT NULL,
                line_item TEXT,
                project_id TEXT,
                PRIMARY KEY (provider, model, start_time, end_time, line_item, project_id)
            );
            CREATE TABLE IF NOT EXISTS imported_chats (
                source_hash TEXT PRIMARY KEY NOT NULL,
                provider TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT,
                updated_at TEXT,
                model TEXT,
                estimated_input_tokens INTEGER NOT NULL,
                estimated_output_tokens INTEGER NOT NULL,
                estimated_cost_usd REAL,
                snippet TEXT
            );
            CREATE TABLE IF NOT EXISTS import_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                source_path TEXT NOT NULL,
                source_hash TEXT NOT NULL,
                imported_count INTEGER NOT NULL,
                skipped_count INTEGER NOT NULL,
                ran_at TEXT NOT NULL
            );
            "#,
        )
        .context("could not initialize database schema")?;

        Ok(Self { conn })
    }

    pub fn upsert_provider_account(&self, provider: Provider, label: &str) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            r#"
            INSERT INTO provider_accounts (provider, label, connected_at, last_synced_at)
            VALUES (?1, ?2, ?3, NULL)
            ON CONFLICT(provider) DO UPDATE SET label = excluded.label
            "#,
            params![provider.as_str(), label, now.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn mark_synced(&self, provider: Provider) -> Result<()> {
        self.conn.execute(
            "UPDATE provider_accounts SET last_synced_at = ?2 WHERE provider = ?1",
            params![provider.as_str(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn insert_usage_buckets(&mut self, buckets: &[UsageBucket]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO usage_buckets
                    (provider, model, start_time, end_time, input_tokens, output_tokens, cached_input_tokens, requests)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(provider, model, start_time, end_time) DO UPDATE SET
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cached_input_tokens = excluded.cached_input_tokens,
                    requests = excluded.requests
                "#,
            )?;
            for bucket in buckets {
                count += stmt.execute(params![
                    bucket.provider.as_str(),
                    bucket.model.as_deref().unwrap_or("unknown"),
                    bucket.start_time.to_rfc3339(),
                    bucket.end_time.to_rfc3339(),
                    bucket.input_tokens,
                    bucket.output_tokens,
                    bucket.cached_input_tokens,
                    bucket.requests,
                ])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn insert_cost_buckets(&mut self, buckets: &[CostBucket]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO cost_buckets
                    (provider, model, start_time, end_time, amount, currency, line_item, project_id)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(provider, model, start_time, end_time, line_item, project_id) DO UPDATE SET
                    amount = excluded.amount,
                    currency = excluded.currency
                "#,
            )?;
            for bucket in buckets {
                count += stmt.execute(params![
                    bucket.provider.as_str(),
                    bucket.model.as_deref().unwrap_or("unknown"),
                    bucket.start_time.to_rfc3339(),
                    bucket.end_time.to_rfc3339(),
                    bucket.amount,
                    bucket.currency,
                    bucket.line_item.as_deref().unwrap_or(""),
                    bucket.project_id.as_deref().unwrap_or(""),
                ])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn insert_imported_chats(
        &mut self,
        provider: ImportProvider,
        source_path: &str,
        source_hash: &str,
        chats: &[ImportedChat],
    ) -> Result<ImportRun> {
        let tx = self.conn.transaction()?;
        let mut imported = 0;
        let mut skipped = 0;
        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR IGNORE INTO imported_chats
                    (source_hash, provider, title, created_at, updated_at, model, estimated_input_tokens,
                     estimated_output_tokens, estimated_cost_usd, snippet)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
            )?;
            for chat in chats {
                let changed = stmt.execute(params![
                    chat.source_hash.as_str(),
                    chat.provider.as_str(),
                    chat.title.as_str(),
                    chat.created_at.map(|value| value.to_rfc3339()),
                    chat.updated_at.map(|value| value.to_rfc3339()),
                    chat.model.as_deref().unwrap_or("unknown"),
                    chat.estimated_input_tokens,
                    chat.estimated_output_tokens,
                    chat.estimated_cost_usd,
                    chat.snippet.as_deref(),
                ])?;
                if changed == 0 {
                    skipped += 1;
                } else {
                    imported += 1;
                }
            }
        }

        let run = ImportRun {
            provider,
            source_path: source_path.to_string(),
            source_hash: source_hash.to_string(),
            imported_count: imported,
            skipped_count: skipped,
            ran_at: Utc::now(),
        };
        tx.execute(
            r#"
            INSERT INTO import_runs
                (provider, source_path, source_hash, imported_count, skipped_count, ran_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                run.provider.as_str(),
                run.source_path.as_str(),
                run.source_hash.as_str(),
                run.imported_count as i64,
                run.skipped_count as i64,
                run.ran_at.to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(run)
    }

    pub fn dashboard(&self) -> Result<Dashboard> {
        let providers = self.provider_accounts()?;
        let models = self.model_summaries()?;
        let recent_chats = self.recent_chats(12)?;
        let import_runs = self.import_runs(10)?;
        let hourly_usage = self.hourly_usage_summaries()?;

        let total_cost_usd = self.conn.query_row(
            "SELECT COALESCE(SUM(amount), 0.0) FROM cost_buckets",
            [],
            |row| row.get(0),
        )?;
        let (api_input_tokens, api_output_tokens, total_requests) = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), COALESCE(SUM(requests), 0) FROM usage_buckets",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        let (chat_input_tokens, chat_output_tokens) = self.conn.query_row(
            "SELECT COALESCE(SUM(estimated_input_tokens), 0), COALESCE(SUM(estimated_output_tokens), 0) FROM imported_chats",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let imported_chats =
            self.conn
                .query_row("SELECT COUNT(*) FROM imported_chats", [], |row| row.get(0))?;

        Ok(Dashboard {
            total_cost_usd,
            total_input_tokens: api_input_tokens + chat_input_tokens,
            total_output_tokens: api_output_tokens + chat_output_tokens,
            total_requests,
            imported_chats,
            providers,
            models,
            recent_chats,
            import_runs,
            hourly_usage,
        })
    }

    pub fn provider_accounts(&self) -> Result<Vec<ProviderAccount>> {
        let mut stmt = self.conn.prepare(
            "SELECT provider, label, connected_at, last_synced_at FROM provider_accounts ORDER BY provider",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProviderAccount {
                provider: parse_provider(row.get::<_, String>(0)?)?,
                label: row.get(1)?,
                connected_at: parse_time(row.get::<_, String>(2)?)?,
                last_synced_at: parse_optional_time(row.get(3)?)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn usage_buckets(&self) -> Result<Vec<UsageBucket>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT provider, model, start_time, end_time, input_tokens, output_tokens, cached_input_tokens, requests
            FROM usage_buckets ORDER BY start_time DESC, provider, model
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(UsageBucket {
                provider: parse_provider(row.get::<_, String>(0)?)?,
                model: row.get(1)?,
                start_time: parse_time(row.get::<_, String>(2)?)?,
                end_time: parse_time(row.get::<_, String>(3)?)?,
                input_tokens: row.get(4)?,
                output_tokens: row.get(5)?,
                cached_input_tokens: row.get(6)?,
                requests: row.get(7)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn cost_buckets(&self) -> Result<Vec<CostBucket>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT provider, model, start_time, end_time, amount, currency, line_item, project_id
            FROM cost_buckets ORDER BY start_time DESC, provider, model
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CostBucket {
                provider: parse_provider(row.get::<_, String>(0)?)?,
                model: row.get(1)?,
                start_time: parse_time(row.get::<_, String>(2)?)?,
                end_time: parse_time(row.get::<_, String>(3)?)?,
                amount: row.get(4)?,
                currency: row.get(5)?,
                line_item: row.get(6)?,
                project_id: row.get(7)?,
            })
        })?;
        collect_rows(rows)
    }

    pub fn imported_chats(&self) -> Result<Vec<ImportedChat>> {
        self.recent_chats(i64::MAX)
    }

    fn model_summaries(&self) -> Result<Vec<ModelSummary>> {
        let mut stmt = self.conn.prepare(
            r#"
            WITH usage_summary AS (
                SELECT provider, COALESCE(model, 'unknown') AS model,
                       SUM(input_tokens) AS input_tokens,
                       SUM(output_tokens) AS output_tokens,
                       SUM(cached_input_tokens) AS cached_input_tokens,
                       SUM(requests) AS requests
                FROM usage_buckets
                GROUP BY provider, COALESCE(model, 'unknown')
            ),
            cost_summary AS (
                SELECT provider, COALESCE(model, 'unknown') AS model, SUM(amount) AS cost_usd
                FROM cost_buckets
                GROUP BY provider, COALESCE(model, 'unknown')
            ),
            chat_summary AS (
                SELECT provider, COALESCE(model, 'unknown') AS model,
                       SUM(estimated_input_tokens) AS input_tokens,
                       SUM(estimated_output_tokens) AS output_tokens,
                       COALESCE(SUM(estimated_cost_usd), 0.0) AS cost_usd,
                       COUNT(*) AS imported_chats
                FROM imported_chats
                GROUP BY provider, COALESCE(model, 'unknown')
            )
            SELECT provider, model,
                   SUM(input_tokens), SUM(output_tokens), SUM(cached_input_tokens), SUM(requests),
                   SUM(cost_usd), SUM(imported_chats)
            FROM (
                SELECT provider, model, input_tokens, output_tokens, cached_input_tokens, requests, 0.0 AS cost_usd, 0 AS imported_chats FROM usage_summary
                UNION ALL
                SELECT provider, model, 0, 0, 0, 0, cost_usd, 0 FROM cost_summary
                UNION ALL
                SELECT provider, model, input_tokens, output_tokens, 0, 0, cost_usd, imported_chats FROM chat_summary
            )
            GROUP BY provider, model
            ORDER BY SUM(cost_usd) DESC, SUM(input_tokens + output_tokens) DESC, model
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ModelSummary {
                provider: row.get(0)?,
                model: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cached_input_tokens: row.get(4)?,
                requests: row.get(5)?,
                cost_usd: row.get(6)?,
                imported_chats: row.get(7)?,
            })
        })?;
        collect_rows(rows)
    }

    fn hourly_usage_summaries(&self) -> Result<Vec<HourlyUsageSummary>> {
        let mut grouped: BTreeMap<(String, u8), HourlyUsageSummary> = BTreeMap::new();

        for bucket in self.usage_buckets()? {
            let hour = bucket.start_time.hour() as u8;
            let entry = hourly_entry(&mut grouped, bucket.provider.as_str(), hour);
            entry.input_tokens += bucket.input_tokens;
            entry.output_tokens += bucket.output_tokens;
            entry.requests += bucket.requests;
        }

        for chat in self.imported_chats()? {
            let Some(time) = chat.created_at.or(chat.updated_at) else {
                continue;
            };
            let hour = time.hour() as u8;
            let entry = hourly_entry(&mut grouped, chat.provider.as_str(), hour);
            entry.input_tokens += chat.estimated_input_tokens;
            entry.output_tokens += chat.estimated_output_tokens;
            entry.imported_chats += 1;
        }

        Ok(grouped.into_values().collect())
    }

    fn recent_chats(&self, limit: i64) -> Result<Vec<ImportedChat>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT provider, title, created_at, updated_at, model, estimated_input_tokens,
                   estimated_output_tokens, estimated_cost_usd, source_hash, snippet
            FROM imported_chats
            ORDER BY COALESCE(updated_at, created_at, '') DESC, title
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(ImportedChat {
                provider: parse_provider(row.get::<_, String>(0)?)?,
                title: row.get(1)?,
                created_at: parse_optional_time(row.get(2)?)?,
                updated_at: parse_optional_time(row.get(3)?)?,
                model: row.get(4)?,
                estimated_input_tokens: row.get(5)?,
                estimated_output_tokens: row.get(6)?,
                estimated_cost_usd: row.get(7)?,
                source_hash: row.get(8)?,
                snippet: row.get(9)?,
            })
        })?;
        collect_rows(rows)
    }

    fn import_runs(&self, limit: i64) -> Result<Vec<ImportRun>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT provider, source_path, source_hash, imported_count, skipped_count, ran_at
            FROM import_runs ORDER BY ran_at DESC LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map([limit], |row| {
            let provider_text: String = row.get(0)?;
            let provider = match provider_text.as_str() {
                "chatgpt" => ImportProvider::ChatGpt,
                "claude" => ImportProvider::Claude,
                _ => ImportProvider::ChatGpt,
            };
            Ok(ImportRun {
                provider,
                source_path: row.get(1)?,
                source_hash: row.get(2)?,
                imported_count: row.get::<_, i64>(3)? as usize,
                skipped_count: row.get::<_, i64>(4)? as usize,
                ran_at: parse_time(row.get::<_, String>(5)?)?,
            })
        })?;
        collect_rows(rows)
    }
}

fn hourly_entry<'a>(
    grouped: &'a mut BTreeMap<(String, u8), HourlyUsageSummary>,
    provider: &str,
    hour_utc: u8,
) -> &'a mut HourlyUsageSummary {
    grouped
        .entry((provider.to_string(), hour_utc))
        .or_insert_with(|| HourlyUsageSummary {
            provider: provider.to_string(),
            hour_utc,
            ..HourlyUsageSummary::default()
        })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn parse_provider(value: String) -> rusqlite::Result<Provider> {
    value.parse::<Provider>().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        )
    })
}

fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn parse_optional_time(value: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    value.map(parse_time).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    #[test]
    fn inserts_imported_chats_once() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("meterline.sqlite3"), "test-key").unwrap();
        let chat = ImportedChat {
            provider: Provider::OpenAi,
            title: "A chat".to_string(),
            created_at: None,
            updated_at: None,
            model: Some("gpt-test".to_string()),
            estimated_input_tokens: 10,
            estimated_output_tokens: 20,
            estimated_cost_usd: None,
            source_hash: "abc".to_string(),
            snippet: Some("hello".to_string()),
        };

        let first = store
            .insert_imported_chats(
                ImportProvider::ChatGpt,
                "export.zip",
                "ziphash",
                &[chat.clone()],
            )
            .unwrap();
        let second = store
            .insert_imported_chats(ImportProvider::ChatGpt, "export.zip", "ziphash", &[chat])
            .unwrap();

        assert_eq!(first.imported_count, 1);
        assert_eq!(second.imported_count, 0);
        assert_eq!(second.skipped_count, 1);
        assert_eq!(store.dashboard().unwrap().imported_chats, 1);
    }

    #[test]
    fn dashboard_groups_imported_tokens_by_hour() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("meterline.sqlite3"), "test-key").unwrap();
        let chat = ImportedChat {
            provider: Provider::Claude,
            title: "Hourly chat".to_string(),
            created_at: Some(Utc.with_ymd_and_hms(2026, 4, 26, 14, 30, 0).unwrap()),
            updated_at: None,
            model: Some("claude-test".to_string()),
            estimated_input_tokens: 40,
            estimated_output_tokens: 15,
            estimated_cost_usd: None,
            source_hash: "hourly".to_string(),
            snippet: None,
        };

        store
            .insert_imported_chats(ImportProvider::Claude, "claude.zip", "ziphash", &[chat])
            .unwrap();

        let dashboard = store.dashboard().unwrap();
        let row = dashboard
            .hourly_usage
            .iter()
            .find(|row| row.provider == "claude" && row.hour_utc == 14)
            .unwrap();
        assert_eq!(row.input_tokens, 40);
        assert_eq!(row.output_tokens, 15);
        assert_eq!(row.imported_chats, 1);
        assert_eq!(dashboard.total_input_tokens, 40);
        assert_eq!(dashboard.total_output_tokens, 15);
    }
}
