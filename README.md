# Meterline

Meterline is a fast, comfy terminal tool for tracking OpenAI and Anthropic usage across models, tokens, live requests, costs, and local activity.

It deliberately uses official surfaces:

- A local OpenAI/Anthropic-compatible proxy for real-time API traffic.
- OpenAI organization usage and cost API endpoints for optional backfill.
- Anthropic Usage and Cost Admin API endpoints for optional organization backfill.
- Optional official ChatGPT and Claude data-export archives for historical metadata only.

Meterline does not scrape logged-in web sessions, automate provider websites, or store provider passwords.

## Support Meterline

If Meterline saves you time and you want to say thanks, you can send a small tip or coffee here:

[ko-fi.com/apolonus](https://ko-fi.com/apolonus)

## Status

This is a v1 implementation with a working local database, CLI, TUI, live proxy, imports, exports, and provider sync clients.

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
meterline connect openai
meterline connect claude
meterline live
meterline daemon
meterline watch
meterline sync
meterline export --format json
meterline export --format csv --output meterline.csv
meterline support
```

`meterline` and `meterline live` run the live proxy and TUI together in one terminal. `meterline daemon` is available for headless/advanced setups.

The live proxy listens on `127.0.0.1:37373`:

```sh
OpenAI base URL:    http://127.0.0.1:37373/openai/v1
Anthropic base URL: http://127.0.0.1:37373/anthropic/v1
```

Point SDKs or tools at those base URLs. Meterline forwards the request to the official provider API, streams the response back, and records provider-returned token usage when available.

Open API-key pages before connecting:

```sh
meterline connect openai --browser
meterline connect claude --browser
```

Optional historical import support remains available, but it is not the primary live path:

```sh
meterline import chatgpt path/to/chatgpt-export.zip
meterline import claude path/to/claude-export.zip
```

## TUI Controls

- `o` opens the OpenAI API-key page.
- `c` opens the Claude API-key page.
- After `o` or `c`, Meterline prompts you to paste the API key and stores it in the OS keychain.
- `r` runs a manual provider sync for optional API-connected accounts.
- `v` toggles optional API refresh polling every 60 seconds when providers are connected.
- `g` opens Settings.
- `m` toggles minimized mode.
- `s` hides or shows usage values for privacy and saves the preference.
- `t` cycles the compact tray metric and saves the preference.
- `h`/`l`, left/right, or `1`-`7` switch panels.
- `q` quits.

Home stays focused on connected providers only. If Claude is connected and OpenAI is not, the main meter shows Claude, live requests, tokens, latest activity, and a compact token graph. Add another provider later from the Providers panel.

Meterline stays terminal-native in v1. The tray is a compact in-terminal status strip rather than an operating-system system tray process, which keeps installation light and predictable across Windows, macOS, and Linux.

Browser setup is browser-assisted, not browser-scraping: Meterline opens official API-key pages and never reads browser cookies, sessions, or passwords.

## Customization

Open the Settings panel with `g`. Meterline saves simple preferences to `settings.json` in the local data directory:

- Theme: `balanced`, `openai`, `claude`, or `mono`.
- Manual sync window: `7`, `31`, or `90` days.
- Startup panel: `home`, `providers`, `chats`, or `imports`.
- Value privacy, default tray metric, and optional API refresh on/off.

## Storage and Privacy

Meterline stores app data in a local SQLite database. Provider API keys are stored in the operating system keychain when available.

The default build favors simple installation and portable prebuilt binaries. Advanced users can build with SQLCipher-backed encrypted storage:

```sh
cargo install --git https://github.com/apolonuss/meterline --locked --no-default-features --features encrypted-storage
```

On Windows, the SQLCipher build uses vendored OpenSSL and requires Perl in addition to the normal Rust/MSVC build tools. The default installer does not require this.

Live proxy activity is metadata-first. Meterline stores provider, endpoint, timestamps, status code, model, request ID, and token counts returned by the provider. It does not store full request or response bodies.

Imported chat history is also metadata-first when used. Meterline stores titles, timestamps, provider, model hints, estimated token counts, source hashes, and optional short snippets. It does not store full message bodies.

Home shows a compact token graph for the active connected provider. The Models panel also shows usage rhythm by hour. For live proxy traffic this is based on provider-returned usage in real time. For optional API sync it includes synced usage buckets too.

Set `METERLINE_HOME` to override the app data directory, which is useful for tests and portable installs.

## Provider Notes

Start live:

```sh
meterline connect openai
meterline connect claude
meterline
```

Meterline can also forward incoming request auth headers, so tools may keep using their own API keys while Meterline acts as the local base URL.

Optional API sync is separate. OpenAI usage sync expects an API key with access to organization usage and costs. Anthropic usage/cost sync expects an Admin API key and organization access. The live proxy path does not require Anthropic Admin API access; it tracks requests that pass through Meterline.

Consumer ChatGPT/Claude web SSO is not used because it does not expose official delegated scopes for terminal apps to read live usage, chats, or remaining quota. Meterline stays honest: real-time tracking works for API traffic routed through the local proxy. Claude's own usage limit can vary by plan, model, message length, attachments, current conversation length, features, and provider capacity; Meterline does not scrape Claude settings.
