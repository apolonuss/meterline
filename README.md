# Meterline

Meterline is a fast, comfy terminal tool for tracking OpenAI/ChatGPT and Anthropic/Claude usage across models, costs, and imported chat metadata.

It deliberately uses official surfaces:

- Official ChatGPT and Claude data-export archives for consumer chat history.
- Optional OpenAI organization usage and cost API endpoints for API users.
- Optional Anthropic Usage and Cost Admin API endpoints for organization API users.

Meterline does not scrape logged-in web sessions, automate provider websites, or store provider passwords.

## Support Meterline

If Meterline saves you time and you want to say thanks, you can send a small tip or coffee here:

[ko-fi.com/apolonus](https://ko-fi.com/apolonus)

## Status

This is a v1 implementation scaffold with a working local database, CLI, TUI, importers, exports, and provider sync clients.

## Install Fast

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/apolonuss/meterline/main/install.ps1 | iex
```

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/apolonuss/meterline/main/install.sh | sh
```

The installer looks for a prebuilt GitHub release for your operating system and CPU. If no release asset is available yet, it falls back to building from source with Cargo.

Windows x64 has a prebuilt release today. macOS and Linux can use the same installer; until native release archives are published, those installs build from source with Cargo.

On Windows, the installer adds Meterline to your user PATH. On macOS and Linux, it installs to `~/.local/bin` and prints the PATH line to add if that directory is not already available.

After installation:

```sh
meterline init
meterline
```

## Other Install Options

Install from source with Cargo:

```sh
cargo install --git https://github.com/apolonuss/meterline --locked
```

Install from a local checkout:

```sh
cargo install --path . --locked
```

## Commands

```sh
meterline
meterline init
meterline connect openai --browser
meterline connect claude --browser
meterline import chatgpt path/to/chatgpt-export.zip
meterline import claude path/to/claude-export.zip
meterline export --format json
meterline export --format csv --output meterline.csv
meterline support
```

Optional API usage sync for users who have provider API/admin access:

```sh
meterline connect openai
meterline connect claude
meterline sync
```

## TUI Controls

- `o` opens ChatGPT Data Controls so individual users can export their official data zip.
- `c` opens Claude Privacy settings so individual users can export their official data zip.
- `r` runs a manual provider sync for optional API-connected accounts.
- `v` toggles live refresh, which polls official authenticated usage APIs every 60 seconds when providers are connected.
- `g` opens Settings.
- `m` toggles minimized mode.
- `s` hides or shows usage values for privacy and saves the preference.
- `t` cycles the compact tray metric and saves the preference.
- `h`/`l`, left/right, or `1`-`7` switch panels.
- `q` quits.

Meterline stays terminal-native in v1. The tray is a compact in-terminal status strip rather than an operating-system system tray process, which keeps installation light and predictable across Windows, macOS, and Linux.

Browser setup is browser-assisted, not browser-scraping: Meterline opens official export/settings pages and never reads browser cookies, sessions, or passwords.

## Customization

Open the Settings panel with `g`. Meterline saves simple preferences to `settings.json` in the local data directory:

- Theme: `balanced`, `openai`, `claude`, or `mono`.
- Manual sync window: `7`, `31`, or `90` days.
- Startup panel: `home`, `providers`, `chats`, or `imports`.
- Value privacy, default tray metric, and live refresh on/off.

## Storage and Privacy

Meterline stores app data in a local SQLite database. Provider API keys are stored in the operating system keychain when available.

The default build favors simple installation and portable prebuilt binaries. Advanced users can build with SQLCipher-backed encrypted storage:

```sh
cargo install --git https://github.com/apolonuss/meterline --locked --no-default-features --features encrypted-storage
```

On Windows, the SQLCipher build uses vendored OpenSSL and requires Perl in addition to the normal Rust/MSVC build tools. The default installer does not require this.

Imported chat history is metadata-first in v1. Meterline stores titles, timestamps, provider, model hints, estimated token counts, source hashes, and optional short snippets. It does not store full message bodies.

The Models panel also shows a usage rhythm by hour. For individual ChatGPT and Claude users this is based on imported export metadata and estimated tokens. For API users it also includes synced usage buckets when available. This is a historical usage pattern, not a live remaining-quota meter.

Set `METERLINE_HOME` to override the app data directory, which is useful for tests and portable installs.

## Provider Notes

Individual users should start with official exports:

- ChatGPT: `meterline connect openai --browser`, export from Data Controls, then `meterline import chatgpt <zip>`.
- Claude: `meterline connect claude --browser`, export from Privacy settings, then `meterline import claude <zip>`.

Optional API sync is separate. OpenAI usage sync expects an API key with access to organization usage and costs. Anthropic API usage sync expects an Admin API key and organization access. Individual Claude users do not need Anthropic Admin API access to use Meterline with exports.

Live refresh uses official authenticated API polling. It does not use provider web sessions, passwords, scraping, or local webhooks. Provider reporting can lag behind actual usage, so Meterline shows the last refresh time in the TUI.

Claude's own usage limit can vary by plan, model, message length, attachments, current conversation length, features, and provider capacity. Claude exposes usage progress to signed-in paid users in Claude settings, but Meterline does not scrape that page. Meterline shows your imported/synced usage by hour so you can see when you tend to spend tokens.
