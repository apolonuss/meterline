use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Store a provider key in the OS keychain.
    Connect {
        provider: Provider,
        #[arg(long, env)]
        key: Option<String>,
    },
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
        Some(Command::Connect { provider, key }) => {
            let key = match key {
                Some(value) => value,
                None => rpassword::prompt_password(format!(
                    "Paste your {} key: ",
                    provider.display_name()
                ))?,
            };
            SecretStore::set_provider_key(provider, key.trim())?;
            store.upsert_provider_account(provider, provider.display_name())?;
            println!("Connected {}", provider.display_name());
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
        None => crate::tui::run(&mut store, &paths.settings_path())?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
