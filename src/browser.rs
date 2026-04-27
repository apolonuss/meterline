use anyhow::{Context, Result};
use std::process::Command;

use crate::models::Provider;

pub fn provider_key_url(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "https://platform.openai.com/api-keys",
        Provider::Claude => "https://console.anthropic.com/settings/keys",
    }
}

pub fn provider_key_note(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => {
            "Create an OpenAI API key, then Meterline can proxy OpenAI API traffic."
        }
        Provider::Claude => {
            "Create a Claude API key, then Meterline can proxy Anthropic API traffic."
        }
    }
}

pub fn provider_proxy_base_url(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "http://127.0.0.1:37373/openai/v1",
        Provider::Claude => "http://127.0.0.1:37373/anthropic/v1",
    }
}

pub fn provider_env_var(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "OPENAI_API_KEY",
        Provider::Claude => "ANTHROPIC_API_KEY",
    }
}

pub fn provider_product_name(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "OpenAI",
        Provider::Claude => "Claude",
    }
}

pub fn open_provider_key_page(provider: Provider) -> Result<&'static str> {
    let url = provider_key_url(provider);
    open_url(url)?;
    Ok(url)
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .with_context(|| format!("could not open browser for {url}"))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .with_context(|| format!("could not open browser for {url}"))?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .with_context(|| format!("could not open browser for {url}"))?;
        Ok(())
    }

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        all(unix, not(target_os = "macos"))
    )))]
    {
        Err(anyhow::anyhow!(
            "opening a browser is not supported on this platform"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_urls_are_official_key_pages() {
        assert_eq!(
            provider_key_url(Provider::OpenAi),
            "https://platform.openai.com/api-keys"
        );
        assert_eq!(
            provider_key_url(Provider::Claude),
            "https://console.anthropic.com/settings/keys"
        );
    }

    #[test]
    fn provider_notes_explain_live_proxy() {
        assert!(provider_key_note(Provider::OpenAi).contains("proxy OpenAI"));
        assert!(provider_key_note(Provider::Claude).contains("proxy Anthropic"));
        assert!(provider_proxy_base_url(Provider::Claude).contains("/anthropic/v1"));
    }

    #[test]
    fn provider_env_vars_are_standard_sdk_names() {
        assert_eq!(provider_env_var(Provider::OpenAi), "OPENAI_API_KEY");
        assert_eq!(provider_env_var(Provider::Claude), "ANTHROPIC_API_KEY");
    }
}
