use anyhow::{Context, Result};
use chrono::{Duration, TimeZone, Utc};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::models::{CostBucket, Provider, UsageBucket};
use crate::providers::SyncReport;
use crate::store::Store;

const BASE_URL: &str = "https://api.openai.com/v1/organization";

pub fn sync(store: &mut Store, api_key: &str, days: i64) -> Result<SyncReport> {
    let client = Client::builder()
        .user_agent("Meterline/0.1.0")
        .build()
        .context("could not create HTTP client")?;
    let end = Utc::now();
    let start = end - Duration::days(days.clamp(1, 31));
    let start_unix = start.timestamp().to_string();
    let end_unix = end.timestamp().to_string();

    let usage_json: Value = client
        .get(format!("{BASE_URL}/usage/completions"))
        .bearer_auth(api_key)
        .query(&[
            ("start_time", start_unix.as_str()),
            ("end_time", end_unix.as_str()),
            ("bucket_width", "1d"),
            ("group_by[]", "model"),
            ("limit", "31"),
        ])
        .send()
        .context("OpenAI usage request failed")?
        .error_for_status()
        .context("OpenAI usage request was rejected")?
        .json()
        .context("OpenAI usage response was not JSON")?;

    let cost_json: Value = client
        .get(format!("{BASE_URL}/costs"))
        .bearer_auth(api_key)
        .query(&[
            ("start_time", start_unix.as_str()),
            ("end_time", end_unix.as_str()),
            ("bucket_width", "1d"),
            ("group_by[]", "line_item"),
            ("limit", "31"),
        ])
        .send()
        .context("OpenAI cost request failed")?
        .error_for_status()
        .context("OpenAI cost request was rejected")?
        .json()
        .context("OpenAI cost response was not JSON")?;

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
        let Some((start_time, end_time)) = openai_bucket_times(bucket) else {
            continue;
        };
        for result in bucket
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            buckets.push(UsageBucket {
                provider: Provider::OpenAi,
                model: string_or_none(result.get("model")),
                start_time,
                end_time,
                input_tokens: int_field(result, "input_tokens"),
                output_tokens: int_field(result, "output_tokens"),
                cached_input_tokens: int_field(result, "input_cached_tokens"),
                requests: int_field(result, "num_model_requests"),
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
        let Some((start_time, end_time)) = openai_bucket_times(bucket) else {
            continue;
        };
        for result in bucket
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let amount = result
                .pointer("/amount/value")
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let currency = result
                .pointer("/amount/currency")
                .and_then(Value::as_str)
                .unwrap_or("usd")
                .to_string();
            buckets.push(CostBucket {
                provider: Provider::OpenAi,
                model: string_or_none(result.get("model")),
                start_time,
                end_time,
                amount,
                currency,
                line_item: string_or_none(result.get("line_item")),
                project_id: string_or_none(result.get("project_id")),
            });
        }
    }
    buckets
}

fn openai_bucket_times(value: &Value) -> Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> {
    let start = value.get("start_time").and_then(Value::as_i64)?;
    let end = value.get("end_time").and_then(Value::as_i64)?;
    Some((
        Utc.timestamp_opt(start, 0).single()?,
        Utc.timestamp_opt(end, 0).single()?,
    ))
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
    fn parses_usage_by_model() {
        let value = json!({
            "data": [{
                "start_time": 1730419200,
                "end_time": 1730505600,
                "results": [{
                    "input_tokens": 5000,
                    "output_tokens": 1000,
                    "input_cached_tokens": 4000,
                    "num_model_requests": 5,
                    "model": "gpt-test"
                }]
            }]
        });
        let buckets = parse_usage(&value);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].model.as_deref(), Some("gpt-test"));
        assert_eq!(buckets[0].input_tokens, 5000);
    }

    #[test]
    fn parses_costs() {
        let value = json!({
            "data": [{
                "start_time": 1730419200,
                "end_time": 1730505600,
                "results": [{
                    "amount": {"value": 0.06, "currency": "usd"},
                    "line_item": "Text tokens"
                }]
            }]
        });
        let buckets = parse_costs(&value);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].amount, 0.06);
        assert_eq!(buckets[0].line_item.as_deref(), Some("Text tokens"));
    }
}
