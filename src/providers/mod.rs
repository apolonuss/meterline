pub mod anthropic;
pub mod openai;

use anyhow::Result;

use crate::models::Provider;
use crate::secrets::SecretStore;
use crate::store::Store;

#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    pub usage_rows: usize,
    pub cost_rows: usize,
}

pub fn sync_provider(store: &mut Store, provider: Provider, days: i64) -> Result<SyncReport> {
    let key = SecretStore::provider_key(provider)?;
    let report = match provider {
        Provider::OpenAi => openai::sync(store, &key, days)?,
        Provider::Claude => anthropic::sync(store, &key, days)?,
    };
    store.mark_synced(provider)?;
    Ok(report)
}
