# Meterline

Meterline is a fast, comfy terminal tool for tracking OpenAI/ChatGPT and Anthropic/Claude usage across models, costs, and imported chat metadata.

It deliberately uses official surfaces:

- OpenAI organization usage and cost API endpoints.
- Anthropic Usage and Cost Admin API endpoints.
- Official ChatGPT and Claude data-export archives for consumer chat history.

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
meterline connect openai
meterline connect claude
meterline sync
meterline import chatgpt path/to/chatgpt-export.zip
meterline import claude path/to/claude-export.zip
meterline export --format json
meterline export --format csv --output meterline.csv
meterline support
```

## TUI Controls

- `m` toggles minimized mode.
- `s` hides or shows usage values for privacy.
- `t` cycles the compact tray metric in the logo and footer.
- `h`/`l`, left/right, or `1`-`6` switch panels.
- `q` quits.

Meterline stays terminal-native in v1. The tray is a compact in-terminal status strip rather than an operating-system system tray process, which keeps installation light and predictable across Windows, macOS, and Linux.

## Storage and Privacy

Meterline stores app data in a local SQLCipher-encrypted SQLite database. The database key and provider API keys are stored in the operating system keychain when available.

Imported chat history is metadata-first in v1. Meterline stores titles, timestamps, provider, model hints, estimated token counts, source hashes, and optional short snippets. It does not store full message bodies.

Set `METERLINE_HOME` to override the app data directory, which is useful for tests and portable installs.

## Provider Notes

OpenAI usage sync expects an API key with access to organization usage and costs. Anthropic usage sync expects an Admin API key beginning with `sk-ant-admin...`; individual Claude users can still import official Claude data exports.
