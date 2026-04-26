# Meterline

Meterline is a fast, comfy terminal tool for tracking OpenAI/ChatGPT and Anthropic/Claude usage across models, costs, and imported chat metadata.

It deliberately uses official surfaces:

- OpenAI organization usage and cost API endpoints.
- Anthropic Usage and Cost Admin API endpoints.
- Official ChatGPT and Claude data-export archives for consumer chat history.

Meterline does not scrape logged-in web sessions, automate provider websites, or store provider passwords.

## Status

This is a v1 implementation scaffold with a working local database, CLI, TUI, importers, exports, and provider sync clients.

## Install

```sh
cargo install --path .
```

## Commands

```sh
meterline
meterline init
meterline connect openai
meterline connect claude
meterline sync
meterline import chatgpt path/to/chatgpt-export.zip
meterline import claude path/to/claude-export.zip
meterline export --format json
meterline export --format csv --output meterline.csv
```

## Storage and Privacy

Meterline stores app data in a local SQLCipher-encrypted SQLite database. The database key and provider API keys are stored in the operating system keychain when available.

Imported chat history is metadata-first in v1. Meterline stores titles, timestamps, provider, model hints, estimated token counts, source hashes, and optional short snippets. It does not store full message bodies.

Set `METERLINE_HOME` to override the app data directory, which is useful for tests and portable installs.

## Provider Notes

OpenAI usage sync expects an API key with access to organization usage and costs. Anthropic usage sync expects an Admin API key beginning with `sk-ant-admin...`; individual Claude users can still import official Claude data exports.
