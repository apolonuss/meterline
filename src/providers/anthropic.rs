use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::models::{CostBucket, Provider, UsageBucket};
use crate::providers::SyncReport;
use crate::store::Store;

const BASE_URL: &str = "https://api.anthropic.com/v1/organizations";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub fn sync(store: &mut Store, api_key: &str, days: i64) -> Result<SyncReport> {
    let client = Client::builder()
        .user_agent("Meterline/0.1.0")
        .build()
        .context("could not create HTTP client")?;
    let end = Utc::now();
    let start = end - Duration::days(days.clamp(1, 31));
    let starting_at = start.to_rfc3339();
    let ending_at = end.to_rfc3339();

    let usage_json: Value = client
        .get(format!("{BASE_URL}/usage_report/messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .query(&[
            ("starting_at", starting_at.as_str()),
            ("ending_at", ending_at.as_str()),
            ("bucket_width", "1d"),
            ("group_by[]", "model"),
            ("limit", "31"),
        ])
        .send()
        .context("Anthropic usage request failed")?
        .error_for_status()
        .context("Anthropic usage request was rejected")?
        .json()
        .context("Anthropic usage response was not JSON")?;

    let cost_json: Value = client
        .get(format!("{BASE_URL}/cost_report"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .query(&[
            ("starting_at", starting_at.as_str()),
            ("ending_at", ending_at.as_str()),
            ("group_by[]", "description"),
            ("limit", "31"),
        ])
        .send()
        .context("Anthropic cost request failed")?
        .error_for_status()
        .context("Anthropic cost request was rejected")?
        .json()
        .context("Anthropic cost response was not JSON")?;

    let usage = parse_usage(&usage_json);
    let costs = parse_costs(&cost_json);
    let usage_rows = store.insert_usage_buckets(&usage)?;
    let cost_rows = store.insert_cost_buckets(&costs)?;
    Ok(SyncReport {
        usage_rows,
        cost_rows,
    })
}

pub fn parse_usage(value: &Value) -> Vec<UsageBucket> {
    let mut buckets = Vec::new();
    for bucket in value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some((start_time, end_time)) = anthropic_bucket_times(bucket) else {
            continue;
        };
        for result in bucket
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            buckets.push(UsageBucket {
                provider: Provider::Claude,
                model: string_or_none(result.get("model")),
                start_time,
                end_time,
                input_tokens: int_field(result, "uncached_input_tokens")
                    + int_field(result, "cache_read_input_tokens")
                    + result
                        .pointer("/cache_creation/ephemeral_1h_input_tokens")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                    + result
                        .pointer("/cache_creation/ephemeral_5m_input_tokens")
                        .and_then(Value::as_i64)
                        .unwrap_or_default(),
                output_tokens: int_field(result, "output_tokens"),
                cached_input_tokens: int_field(result, "cache_read_input_tokens"),
                requests: 0,
            });
        }
    }
    buckets
}

pub fn parse_costs(value: &Value) -> Vec<CostBucket> {
    let mut buckets = Vec::new();
    for bucket in value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some((start_time, end_time)) = anthropic_bucket_times(bucket) else {
            continue;
        };
        for result in bucket
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let amount = result
                .get("amount")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<f64>().ok())
                .or_else(|| result.get("amount").and_then(Value::as_f64))
                .unwrap_or_default();
            buckets.push(CostBucket {
                provider: Provider::Claude,
                model: string_or_none(result.get("model")),
                start_time,
                end_time,
                amount,
                currency: result
                    .get("currency")
                    .and_then(Value::as_str)
                    .unwrap_or("USD")
                    .to_ascii_lowercase(),
                line_item: string_or_none(result.get("description")),
                project_id: string_or_none(result.get("workspace_id")),
            });
        }
    }
    buckets
}

fn anthropic_bucket_times(value: &Value) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start = parse_time(value.get("starting_at")?.as_str()?)?;
    let end = parse_time(value.get("ending_at")?.as_str()?)?;
    Some((start, end))
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .ok()
}

fn int_field(value: &Value, name: &str) -> i64 {
    value.get(name).and_then(Value::as_i64).unwrap_or_default()
}

fn string_or_none(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_usage_report() {
        let value = json!({
            "data": [{
                "starting_at": "2025-08-01T00:00:00Z",
                "ending_at": "2025-08-02T00:00:00Z",
                "results": [{
                    "uncached_input_tokens": 1500,
                    "cache_creation": {
                        "ephemeral_1h_input_tokens": 1000,
                        "ephemeral_5m_input_tokens": 500
                    },
                    "cache_read_input_tokens": 200,
                    "output_tokens": 500,
                    "model": "claude-test"
                }]
            }]
        });
        let buckets = parse_usage(&value);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].input_tokens, 3200);
        assert_eq!(buckets[0].output_tokens, 500);
    }

    #[test]
    fn parses_cost_report() {
        let value = json!({
            "data": [{
                "starting_at": "2025-08-01T00:00:00Z",
                "ending_at": "2025-08-02T00:00:00Z",
                "results": [{
                    "currency": "USD",
                    "amount": "123.78912",
                    "description": "Claude Sonnet Usage",
                    "model": "claude-test"
                }]
            }]
        });
        let buckets = parse_costs(&value);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].amount, 123.78912);
    }
}
