use anyhow::{Context, Result, anyhow};
use keyring::Entry;
use rand::RngCore;

use crate::models::Provider;

const SERVICE: &str = "meterline";
const DB_KEY_ACCOUNT: &str = "local-db-key-v1";

pub struct SecretStore;

impl SecretStore {
    pub fn database_key() -> Result<String> {
        if let Ok(value) = std::env::var("METERLINE_DB_KEY") {
            if !value.trim().is_empty() {
                return Ok(value);
            }
        }

        let entry = Entry::new(SERVICE, DB_KEY_ACCOUNT).map_err(|err| {
            anyhow!("could not open OS keychain for Meterline database key: {err}")
        })?;

        match entry.get_password() {
            Ok(value) if !value.trim().is_empty() => Ok(value),
            Ok(_) | Err(_) => {
                let mut bytes = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut bytes);
                let key = hex::encode(bytes);
                entry
                    .set_password(&key)
                    .context("could not store Meterline database key in the OS keychain")?;
                Ok(key)
            }
        }
    }

    pub fn set_provider_key(provider: Provider, key: &str) -> Result<()> {
        let entry = provider_entry(provider)?;
        entry.set_password(key).with_context(|| {
            format!(
                "could not store {} key in the OS keychain",
                provider.display_name()
            )
        })
    }

    pub fn provider_key(provider: Provider) -> Result<String> {
        provider_entry(provider)?.get_password().with_context(|| {
            format!(
                "{} is not connected yet; run `meterline connect {}`",
                provider.display_name(),
                provider
            )
        })
    }
}

fn provider_entry(provider: Provider) -> Result<Entry> {
    Entry::new(SERVICE, &format!("provider-{}-api-key", provider.as_str())).map_err(|err| {
        anyhow!(
            "could not open OS keychain for {}: {err}",
            provider.display_name()
        )
    })
}
