use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::browser::{
    open_provider_key_page, provider_env_var, provider_key_note, provider_key_url,
    provider_product_name, provider_proxy_base_url,
};
use crate::connect::{connect_provider_with_key, key_was_cancelled, provider_key_from_env};
use crate::export::{ExportFormat, export_store};
use crate::importers::import_archive;
use crate::models::{ImportProvider, Provider};
use crate::paths::AppPaths;
use crate::providers::sync_provider;
use crate::secrets::SecretStore;
use crate::store::Store;

#[derive(Debug, Parser)]
#[command(
    name = "meterline",
    version,
    about = "Track AI usage from a comfy terminal."
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, env = "METERLINE_HOME", global = true)]
    home: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize the local database.
    Init,
    /// Store a provider API key in the OS keychain.
    Connect {
        provider: Provider,
        #[arg(long)]
        key: Option<String>,
        /// Read the provider key from OPENAI_API_KEY or ANTHROPIC_API_KEY.
        #[arg(long)]
        from_env: bool,
        /// Open the official provider API-key page before prompting.
        #[arg(long)]
        browser: bool,
    },
    /// Start the local always-on live usage proxy.
    Daemon {
        #[arg(long, default_value = "127.0.0.1:37373")]
        bind: String,
    },
    /// Start the live proxy and TUI in one terminal.
    Live {
        #[arg(long, default_value = "127.0.0.1:37373")]
        bind: String,
    },
    /// Launch the TUI live monitor.
    Watch,
    /// Sync official API usage and costs for connected providers.
    Sync {
        #[arg(long)]
        provider: Option<Provider>,
        #[arg(long, default_value_t = 31)]
        days: i64,
    },
    /// Import an official ChatGPT or Claude data-export zip.
    Import {
        provider: ImportProvider,
        zip: PathBuf,
    },
    /// Export local Meterline data.
    Export {
        #[arg(long, value_enum, default_value_t = ExportFormat::Json)]
        format: ExportFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Print local storage paths.
    Paths,
    /// Show the link for sending a thank-you tip.
    Support,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    run_cli(cli)
}

pub fn run_cli(cli: Cli) -> Result<()> {
    if let Some(Command::Support) = &cli.command {
        println!("Thanks for wanting to support Meterline.");
        println!("Tip jar: {}", crate::SUPPORT_URL);
        return Ok(());
    }

    let paths = cli
        .home
        .map(AppPaths::from_dir)
        .map(Ok)
        .unwrap_or_else(AppPaths::discover)?;
    paths.ensure()?;
    if let Some(Command::Paths) = &cli.command {
        println!("data: {}", paths.data_dir().display());
        println!("database: {}", paths.database_path().display());
        println!("settings: {}", paths.settings_path().display());
        return Ok(());
    }

    let db_key = if cfg!(feature = "encrypted-storage") {
        SecretStore::database_key()?
    } else {
        String::new()
    };
    let mut store = Store::open(&paths.database_path(), &db_key)?;

    match cli.command {
        Some(Command::Init) => {
            println!(
                "Meterline initialized at {}",
                paths.database_path().display()
            );
        }
        Some(Command::Connect {
            provider,
            key,
            from_env,
            browser,
        }) => {
            if browser && !from_env && key.is_none() {
                match open_provider_key_page(provider) {
                    Ok(url) => {
                        println!(
                            "Opened {} API key page: {url}",
                            provider_product_name(provider)
                        )
                    }
                    Err(err) => eprintln!(
                        "Could not open browser automatically: {err:#}\nOpen manually: {}",
                        provider_key_url(provider)
                    ),
                }
                println!("{}", provider_key_note(provider));
                println!();
                println!(
                    "Live base URL after `meterline daemon`: {}",
                    provider_proxy_base_url(provider)
                );
            }
            let mut used_env_key = false;
            let key = match key {
                Some(value) => value,
                None if from_env => {
                    used_env_key = true;
                    provider_key_from_env(provider).ok_or_else(|| {
                        anyhow::anyhow!(
                            "{} is not set in this terminal. Set it first or run `meterline connect {}` to paste a key.",
                            provider_env_var(provider),
                            provider
                        )
                    })?
                }
                None => rpassword::prompt_password(format!(
                    "Paste your {} key: ",
                    provider.display_name()
                ))?,
            };
            let key = key.trim();
            if key_was_cancelled(key) {
                println!(
                    "{} connection cancelled. No key was stored.",
                    provider.display_name()
                );
                return Ok(());
            }

            connect_provider_with_key(&mut store, provider, key)?;
            println!("Connected {}", provider.display_name());
            if used_env_key {
                println!("Used {} from this terminal.", provider_env_var(provider));
            }
            println!("Start live tracking with: meterline");
            println!("Base URL: {}", provider_proxy_base_url(provider));
        }
        Some(Command::Daemon { bind }) => {
            drop(store);
            crate::proxy::run(crate::proxy::ProxyConfig {
                bind,
                database_path: paths.database_path(),
                db_key,
            })?;
        }
        Some(Command::Live { bind }) => {
            run_live_tui(&mut store, &paths, &db_key, bind)?;
        }
        Some(Command::Sync { provider, days }) => {
            let providers = provider
                .map(|value| vec![value])
                .unwrap_or_else(|| vec![Provider::OpenAi, Provider::Claude]);
            for provider in providers {
                match sync_provider(&mut store, provider, days) {
                    Ok(report) => println!(
                        "{} synced: {} usage rows, {} cost rows",
                        provider.display_name(),
                        report.usage_rows,
                        report.cost_rows
                    ),
                    Err(err) => eprintln!("{} skipped: {err:#}", provider.display_name()),
                }
            }
        }
        Some(Command::Import { provider, zip }) => {
            let archive = import_archive(provider, &zip)?;
            let run = store.insert_imported_chats(
                provider,
                &zip.display().to_string(),
                &archive.source_hash,
                &archive.chats,
            )?;
            println!(
                "{} import complete: {} imported, {} skipped",
                provider, run.imported_count, run.skipped_count
            );
        }
        Some(Command::Export { format, output }) => {
            export_store(&store, format, output.as_deref())?;
        }
        Some(Command::Paths) => unreachable!("paths exits before opening local storage"),
        Some(Command::Support) => unreachable!("support exits before opening local storage"),
        Some(Command::Watch) => crate::tui::run(&mut store, &paths.settings_path())?,
        None => {
            run_live_tui(&mut store, &paths, &db_key, "127.0.0.1:37373".to_string())?;
        }
    }

    Ok(())
}

fn run_live_tui(store: &mut Store, paths: &AppPaths, db_key: &str, bind: String) -> Result<()> {
    match crate::proxy::spawn(crate::proxy::ProxyConfig {
        bind: bind.clone(),
        database_path: paths.database_path(),
        db_key: db_key.to_string(),
    }) {
        Ok(handle) => {
            eprintln!("Meterline live proxy is running on http://{}", handle.bind);
        }
        Err(err) => {
            eprintln!("Meterline live proxy was not started: {err:#}");
            eprintln!("If another Meterline window is already running, this is usually fine.");
        }
    }
    crate::tui::run(store, &paths.settings_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};
    use tempfile::tempdir;

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn connect_with_empty_key_is_cancelled() {
        let dir = tempdir().unwrap();
        let cli = Cli::try_parse_from([
            "meterline",
            "--home",
            dir.path().to_str().unwrap(),
            "connect",
            "claude",
            "--key",
            "",
        ])
        .unwrap();

        run_cli(cli).unwrap();
        let store = Store::open(&dir.path().join("meterline.sqlite3"), "test-key").unwrap();
        assert!(store.provider_accounts().unwrap().is_empty());
    }
}
