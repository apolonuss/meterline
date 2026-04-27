use anyhow::{Context, Result};
use std::process::Command;

use crate::models::Provider;

pub fn provider_export_url(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "https://chatgpt.com/#settings/DataControls",
        Provider::Claude => "https://claude.ai/settings/privacy",
    }
}

pub fn provider_export_note(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => {
            "ChatGPT individual users can export data from Data Controls, then import the zip."
        }
        Provider::Claude => {
            "Claude individual users can export data from Privacy settings, then import the zip."
        }
    }
}

pub fn provider_import_command(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "meterline import chatgpt path/to/chatgpt-export.zip",
        Provider::Claude => "meterline import claude path/to/claude-export.zip",
    }
}

pub fn provider_product_name(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "ChatGPT",
        Provider::Claude => "Claude",
    }
}

pub fn open_provider_export_page(provider: Provider) -> Result<&'static str> {
    let url = provider_export_url(provider);
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
    fn provider_urls_are_official_export_pages() {
        assert_eq!(
            provider_export_url(Provider::OpenAi),
            "https://chatgpt.com/#settings/DataControls"
        );
        assert_eq!(
            provider_export_url(Provider::Claude),
            "https://claude.ai/settings/privacy"
        );
    }

    #[test]
    fn provider_notes_explain_individual_exports() {
        assert!(provider_export_note(Provider::OpenAi).contains("export data"));
        assert!(provider_export_note(Provider::Claude).contains("Privacy settings"));
        assert!(provider_import_command(Provider::Claude).contains("import claude"));
    }
}
