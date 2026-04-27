use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use reqwest::blocking::Client;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::models::{LiveRequest, Provider};
use crate::secrets::SecretStore;
use crate::store::Store;

const OPENAI_BASE: &str = "https://api.openai.com";
const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_PARSE_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub bind: String,
    pub database_path: PathBuf,
    pub db_key: String,
}

#[derive(Clone, Debug)]
pub struct ProxyHandle {
    pub bind: String,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Default)]
struct UsageCapture {
    model: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
}

pub fn run(config: ProxyConfig) -> Result<()> {
    let listener = TcpListener::bind(&config.bind)
        .with_context(|| format!("could not bind {}", config.bind))?;
    println!("Meterline live proxy listening on http://{}", config.bind);
    println!("OpenAI base URL:    http://{}/openai/v1", config.bind);
    println!("Anthropic base URL: http://{}/anthropic/v1", config.bind);
    println!("Use Ctrl+C to stop.");

    let client = proxy_client()?;

    accept_loop(listener, config, client)
}

pub fn spawn(config: ProxyConfig) -> Result<ProxyHandle> {
    let listener = TcpListener::bind(&config.bind)
        .with_context(|| format!("could not bind {}", config.bind))?;
    let client = proxy_client()?;
    let bind = config.bind.clone();

    thread::Builder::new()
        .name("meterline-live-proxy".to_string())
        .spawn(move || {
            if let Err(err) = accept_loop(listener, config, client) {
                eprintln!("meterline live proxy stopped: {err:#}");
            }
        })
        .context("could not start live proxy thread")?;

    Ok(ProxyHandle { bind })
}

fn accept_loop(listener: TcpListener, config: ProxyConfig, client: Client) -> Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let config = config.clone();
                let client = client.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, config, client) {
                        eprintln!("meterline proxy connection failed: {err:#}");
                    }
                });
            }
            Err(err) => eprintln!("meterline proxy accept failed: {err:#}"),
        }
    }
    Ok(())
}

fn proxy_client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("Meterline/{}", env!("CARGO_PKG_VERSION")))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .timeout(Duration::from_secs(600))
        .build()
        .context("could not create proxy HTTP client")
}

fn handle_connection(mut stream: TcpStream, config: ProxyConfig, client: Client) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(600)))?;

    let Some(request) = read_request(&mut stream)? else {
        return Ok(());
    };

    if request.method == "GET" && request.path == "/health" {
        return write_json(
            &mut stream,
            200,
            r#"{"ok":true,"service":"meterline","mode":"live-proxy"}"#,
        );
    }

    let Some((provider, upstream_path)) = route_path(&request.path) else {
        return write_json(
            &mut stream,
            404,
            r#"{"error":"Use /openai/v1/... or /anthropic/v1/... as the base URL."}"#,
        );
    };

    let started_at = Utc::now();
    let upstream_url = upstream_url(provider, &upstream_path);
    match forward_request(&client, &request, provider, &upstream_url) {
        Ok(response) => stream_response(
            &mut stream,
            response,
            &config,
            provider,
            &request,
            &upstream_path,
            started_at,
        ),
        Err(err) => {
            let finished_at = Utc::now();
            record_live_request(
                &config,
                LiveRequest {
                    provider,
                    method: request.method.clone(),
                    path: upstream_path,
                    model: request_model(&request.body),
                    started_at,
                    finished_at,
                    status_code: 502,
                    input_tokens: 0,
                    output_tokens: 0,
                    cached_input_tokens: 0,
                    request_id: None,
                    error: Some(err.to_string()),
                },
            );
            write_json(
                &mut stream,
                502,
                &format!(
                    r#"{{"error":"Meterline could not reach upstream provider: {}"}}"#,
                    escape_json(&err.to_string())
                ),
            )
        }
    }
}

fn forward_request(
    client: &Client,
    request: &HttpRequest,
    provider: Provider,
    upstream_url: &str,
) -> Result<reqwest::blocking::Response> {
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .with_context(|| format!("unsupported method {}", request.method))?;
    let mut builder = client
        .request(method, upstream_url)
        .body(request.body.clone());

    for (name, value) in &request.headers {
        let lower = name.to_ascii_lowercase();
        if is_hop_by_hop_header(&lower) || lower == "host" || lower == "accept-encoding" {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder = builder.header("accept-encoding", "identity");

    match provider {
        Provider::OpenAi => {
            if !has_header(request, "authorization") {
                let key = SecretStore::provider_key(provider)
                    .context("OpenAI key missing. Run `meterline connect openai`, or send an Authorization header through the proxy.")?;
                builder = builder.header("authorization", format!("Bearer {key}"));
            }
        }
        Provider::Claude => {
            if !has_header(request, "x-api-key") && !has_header(request, "authorization") {
                let key = SecretStore::provider_key(provider)
                    .context("Claude API key missing. Run `meterline connect claude`, or send an x-api-key header through the proxy.")?;
                builder = builder.header("x-api-key", key);
            }
            if !has_header(request, "anthropic-version") {
                builder = builder.header("anthropic-version", DEFAULT_ANTHROPIC_VERSION);
            }
        }
    }

    builder
        .send()
        .with_context(|| format!("upstream request failed for {upstream_url}"))
}

fn stream_response(
    stream: &mut TcpStream,
    mut response: reqwest::blocking::Response,
    config: &ProxyConfig,
    provider: Provider,
    request: &HttpRequest,
    upstream_path: &str,
    started_at: chrono::DateTime<Utc>,
) -> Result<()> {
    let status = response.status();
    let status_code = status.as_u16() as i64;
    let response_headers = response.headers().clone();
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("OK")
    );
    for (name, value) in &response_headers {
        let lower = name.as_str().to_ascii_lowercase();
        if is_hop_by_hop_header(&lower)
            || lower == "content-length"
            || lower == "transfer-encoding"
            || lower == "content-encoding"
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            head.push_str(name.as_str());
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;

    let mut sample = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        stream.write_all(&buffer[..read])?;
        if sample.len() < MAX_PARSE_BODY {
            let remaining = MAX_PARSE_BODY - sample.len();
            sample.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }
    stream.flush()?;

    let finished_at = Utc::now();
    let capture = capture_usage(provider, &request.body, &sample);
    record_live_request(
        config,
        LiveRequest {
            provider,
            method: request.method.clone(),
            path: upstream_path.to_string(),
            model: capture.model,
            started_at,
            finished_at,
            status_code,
            input_tokens: capture.input_tokens,
            output_tokens: capture.output_tokens,
            cached_input_tokens: capture.cached_input_tokens,
            request_id: response_headers
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
            error: if status.is_success() {
                None
            } else {
                Some(status.to_string())
            },
        },
    );

    Ok(())
}

fn record_live_request(config: &ProxyConfig, request: LiveRequest) {
    match Store::open(&config.database_path, &config.db_key)
        .and_then(|mut store| store.insert_live_request(&request))
    {
        Ok(()) => {}
        Err(err) => eprintln!("meterline could not record live request: {err:#}"),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            bail!("connection closed before headers completed");
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
        if buffer.len() > 128 * 1024 {
            bail!("request headers are too large");
        }
    };

    let header_text = std::str::from_utf8(&buffer[..header_end])
        .context("request headers were not valid UTF-8")?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| anyhow!("missing request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| anyhow!("missing method"))?
        .to_string();
    let path = request_parts
        .next()
        .ok_or_else(|| anyhow!("missing path"))?
        .to_string();

    let mut headers = Vec::new();
    let mut content_length = 0_usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().unwrap_or_default();
        }
        headers.push((name.trim().to_string(), value));
    }

    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            bail!("connection closed before body completed");
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer[body_start..body_start + content_length].to_vec();

    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
    }))
}

fn route_path(path: &str) -> Option<(Provider, String)> {
    let (prefix, provider) = if path == "/openai" || path.starts_with("/openai/") {
        ("/openai", Provider::OpenAi)
    } else if path == "/anthropic" || path.starts_with("/anthropic/") {
        ("/anthropic", Provider::Claude)
    } else {
        return None;
    };

    let rest = path.strip_prefix(prefix).unwrap_or_default();
    let upstream_path = if rest.is_empty() { "/v1" } else { rest };
    Some((provider, upstream_path.to_string()))
}

fn upstream_url(provider: Provider, upstream_path: &str) -> String {
    let base = match provider {
        Provider::OpenAi => OPENAI_BASE,
        Provider::Claude => ANTHROPIC_BASE,
    };
    format!("{base}{upstream_path}")
}

fn capture_usage(provider: Provider, request_body: &[u8], response_body: &[u8]) -> UsageCapture {
    let mut capture = UsageCapture {
        model: request_model(request_body),
        ..UsageCapture::default()
    };

    if let Ok(value) = serde_json::from_slice::<Value>(response_body) {
        capture_from_value(provider, &value, &mut capture);
        return capture;
    }

    let text = String::from_utf8_lossy(response_body);
    for line in text.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(data) {
            capture_from_value(provider, &value, &mut capture);
        }
    }
    capture
}

fn capture_from_value(provider: Provider, value: &Value, capture: &mut UsageCapture) {
    if capture.model.is_none() {
        capture.model = find_string_field(value, "model");
    }
    let Some(usage) = find_field(value, "usage") else {
        return;
    };

    match provider {
        Provider::OpenAi => {
            capture.input_tokens = int_field(usage, "input_tokens")
                .or_else(|| int_field(usage, "prompt_tokens"))
                .unwrap_or(capture.input_tokens);
            capture.output_tokens = int_field(usage, "output_tokens")
                .or_else(|| int_field(usage, "completion_tokens"))
                .unwrap_or(capture.output_tokens);
            capture.cached_input_tokens = usage
                .pointer("/input_tokens_details/cached_tokens")
                .and_then(Value::as_i64)
                .or_else(|| {
                    usage
                        .pointer("/prompt_tokens_details/cached_tokens")
                        .and_then(Value::as_i64)
                })
                .unwrap_or(capture.cached_input_tokens);
        }
        Provider::Claude => {
            let cache_creation = int_field(usage, "cache_creation_input_tokens").unwrap_or(0)
                + usage
                    .pointer("/cache_creation/ephemeral_1h_input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                + usage
                    .pointer("/cache_creation/ephemeral_5m_input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
            let cache_read = int_field(usage, "cache_read_input_tokens").unwrap_or(0);
            capture.input_tokens = int_field(usage, "input_tokens").unwrap_or(capture.input_tokens)
                + cache_creation
                + cache_read;
            capture.output_tokens =
                int_field(usage, "output_tokens").unwrap_or(capture.output_tokens);
            capture.cached_input_tokens = cache_read;
        }
    }
}

fn request_model(body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    value
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn find_field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(value) = map.get(name) {
                return Some(value);
            }
            map.values().find_map(|value| find_field(value, name))
        }
        Value::Array(items) => items.iter().find_map(|value| find_field(value, name)),
        _ => None,
    }
}

fn find_string_field(value: &Value, name: &str) -> Option<String> {
    find_field(value, name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn int_field(value: &Value, name: &str) -> Option<i64> {
    value.get(name).and_then(Value::as_i64)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn has_header(request: &HttpRequest, name: &str) -> bool {
    request
        .headers
        .iter()
        .any(|(header, _)| header.eq_ignore_ascii_case(name))
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "upgrade"
    )
}

fn write_json(stream: &mut TcpStream, status: u16, body: &str) -> Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn routes_provider_prefixed_paths() {
        let (provider, path) = route_path("/openai/v1/responses?limit=1").unwrap();
        assert_eq!(provider, Provider::OpenAi);
        assert_eq!(path, "/v1/responses?limit=1");

        let (provider, path) = route_path("/anthropic/v1/messages").unwrap();
        assert_eq!(provider, Provider::Claude);
        assert_eq!(path, "/v1/messages");

        assert!(route_path("/v1/messages").is_none());
    }

    #[test]
    fn captures_openai_usage_json() {
        let request = br#"{"model":"gpt-test"}"#;
        let response = json!({
            "model": "gpt-test",
            "usage": {
                "input_tokens": 120,
                "output_tokens": 40,
                "input_tokens_details": {"cached_tokens": 20}
            }
        });

        let capture = capture_usage(
            Provider::OpenAi,
            request,
            serde_json::to_vec(&response).unwrap().as_slice(),
        );
        assert_eq!(capture.model.as_deref(), Some("gpt-test"));
        assert_eq!(capture.input_tokens, 120);
        assert_eq!(capture.output_tokens, 40);
        assert_eq!(capture.cached_input_tokens, 20);
    }

    #[test]
    fn captures_claude_usage_json() {
        let request = br#"{"model":"claude-test"}"#;
        let response = json!({
            "model": "claude-test",
            "usage": {
                "input_tokens": 100,
                "cache_creation_input_tokens": 10,
                "cache_read_input_tokens": 30,
                "output_tokens": 50
            }
        });

        let capture = capture_usage(
            Provider::Claude,
            request,
            serde_json::to_vec(&response).unwrap().as_slice(),
        );
        assert_eq!(capture.model.as_deref(), Some("claude-test"));
        assert_eq!(capture.input_tokens, 140);
        assert_eq!(capture.output_tokens, 50);
        assert_eq!(capture.cached_input_tokens, 30);
    }

    #[test]
    fn captures_sse_usage() {
        let request = br#"{"model":"gpt-stream"}"#;
        let response = br#"data: {"choices":[]}
data: {"usage":{"prompt_tokens":7,"completion_tokens":9}}
data: [DONE]
"#;

        let capture = capture_usage(Provider::OpenAi, request, response);
        assert_eq!(capture.model.as_deref(), Some("gpt-stream"));
        assert_eq!(capture.input_tokens, 7);
        assert_eq!(capture.output_tokens, 9);
    }
}
