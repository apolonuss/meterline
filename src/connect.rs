use anyhow::{Result, bail};

use crate::browser::provider_env_var;
use crate::models::Provider;
use crate::secrets::SecretStore;
use crate::store::Store;

pub fn provider_key_from_env(provider: Provider) -> Option<String> {
    provider_key_from_env_with(provider, |name| std::env::var(name).ok())
}

pub fn provider_key_from_env_with<F>(provider: Provider, mut lookup: F) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    lookup(provider_env_var(provider))
        .map(|value| value.trim().to_string())
        .filter(|value| !key_was_cancelled(value))
}

pub fn connect_provider_with_key(store: &mut Store, provider: Provider, key: &str) -> Result<()> {
    let key = key.trim();
    if key_was_cancelled(key) {
        bail!("empty key");
    }

    SecretStore::set_provider_key(provider, key)?;
    store.upsert_provider_account(provider, provider.display_name())?;
    Ok(())
}

pub fn key_was_cancelled(key: &str) -> bool {
    key.trim().is_empty() || key.trim() == "\u{1b}"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_uses_provider_specific_variable() {
        let value = provider_key_from_env_with(Provider::Claude, |name| {
            (name == "ANTHROPIC_API_KEY").then(|| "  sk-ant-test  ".to_string())
        });

        assert_eq!(value.as_deref(), Some("sk-ant-test"));
    }

    #[test]
    fn env_key_ignores_empty_values() {
        let value = provider_key_from_env_with(Provider::OpenAi, |_| Some("   ".to_string()));
        assert!(value.is_none());
    }

    #[test]
    fn escaped_or_empty_key_counts_as_cancelled() {
        assert!(key_was_cancelled(""));
        assert!(key_was_cancelled("   "));
        assert!(key_was_cancelled("\u{1b}"));
        assert!(!key_was_cancelled("sk-test"));
    }
}
