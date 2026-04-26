use anyhow::{Context, Result};
use std::process::Command;

use crate::models::Provider;

pub fn provider_connect_url(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "https://platform.openai.com/api-keys",
        Provider::Claude => "https://console.anthropic.com/settings/admin-keys",
    }
}

pub fn open_provider_connect_page(provider: Provider) -> Result<&'static str> {
    let url = provider_connect_url(provider);
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
    fn provider_urls_are_official_console_pages() {
        assert_eq!(
            provider_connect_url(Provider::OpenAi),
            "https://platform.openai.com/api-keys"
        );
        assert_eq!(
            provider_connect_url(Provider::Claude),
            "https://console.anthropic.com/settings/admin-keys"
        );
    }
}
