use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::browser::{
    open_provider_key_page, provider_key_note, provider_key_url, provider_product_name,
    provider_proxy_base_url,
};
use crate::models::{Dashboard, HourlyUsageSummary, Provider, ProviderAccount};
use crate::providers::sync_provider;
use crate::secrets::SecretStore;
use crate::settings::{AppSettings, Theme};
use crate::store::Store;

const TABS: [&str; 7] = [
    "Home",
    "Models",
    "Providers",
    "Chats",
    "Imports",
    "Exports",
    "Settings",
];
const LIVE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
struct Palette {
    openai: Color,
    claude: Color,
    highlight: Color,
    value: Color,
    border: Color,
    soft: Color,
    warning: Color,
}

#[derive(Clone, Debug)]
struct TuiState {
    selected: usize,
    tray_item: usize,
    minimized: bool,
    hide_values: bool,
    notice: String,
    settings: AppSettings,
    last_live_refresh: Option<DateTime<Utc>>,
}

impl TuiState {
    fn from_settings(settings: AppSettings) -> Self {
        Self {
            selected: settings.startup_panel.index(),
            tray_item: settings.default_tray_metric.index(),
            minimized: false,
            hide_values: settings.hide_values,
            notice:
                "Live meter ready. Connect a provider, then route SDK traffic through Meterline."
                    .to_string(),
            settings,
            last_live_refresh: None,
        }
    }

    fn palette(&self) -> Palette {
        palette_for(self.settings.theme)
    }

    fn save_settings(&mut self, settings_path: &Path, message: impl Into<String>) {
        match self.settings.save(settings_path) {
            Ok(()) => self.notice = message.into(),
            Err(err) => self.notice = format!("Settings could not be saved: {err:#}"),
        }
    }
}

impl Default for TuiState {
    fn default() -> Self {
        Self::from_settings(AppSettings::default())
    }
}

pub fn run(store: &mut Store, settings_path: &Path) -> Result<()> {
    let settings = AppSettings::load(settings_path)?;
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, store, settings_path, settings);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn suspend_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    store: &mut Store,
    settings_path: &Path,
    settings: AppSettings,
) -> Result<()> {
    let mut state = TuiState::from_settings(settings);
    let mut dashboard = store.dashboard()?;
    let mut last_live_poll: Option<Instant> = None;
    let mut last_dashboard_reload = Instant::now();

    loop {
        if last_dashboard_reload.elapsed() >= Duration::from_secs(1) {
            dashboard = store.dashboard()?;
            last_dashboard_reload = Instant::now();
        }

        if live_refresh_due(&state, &dashboard, last_live_poll) {
            state.notice = "Live refresh running from official provider APIs...".to_string();
            terminal.draw(|frame| render_app(frame, &dashboard, &state, settings_path))?;
            state.notice = sync_connected(store, &dashboard, 1, "Live");
            state.last_live_refresh = Some(Utc::now());
            dashboard = store.dashboard()?;
            last_live_poll = Some(Instant::now());
            last_dashboard_reload = Instant::now();
        }

        terminal.draw(|frame| render_app(frame, &dashboard, &state, settings_path))?;
        if event::poll(Duration::from_millis(160))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('m') => state.minimized = !state.minimized,
                    KeyCode::Enter if state.minimized => state.minimized = false,
                    KeyCode::Char('s') => {
                        state.hide_values = !state.hide_values;
                        state.settings.hide_values = state.hide_values;
                        state.save_settings(
                            settings_path,
                            if state.hide_values {
                                "Values hidden by default."
                            } else {
                                "Values visible by default."
                            },
                        );
                    }
                    KeyCode::Char('t') | KeyCode::Tab => {
                        state.settings.cycle_tray_metric();
                        state.tray_item = state.settings.default_tray_metric.index();
                        state.save_settings(settings_path, "Default tray metric saved.");
                    }
                    KeyCode::Char('y') if !state.minimized => {
                        state.settings.cycle_theme();
                        state.save_settings(settings_path, "Theme saved.");
                    }
                    KeyCode::Char('d') if !state.minimized => {
                        state.settings.cycle_sync_days();
                        state.save_settings(settings_path, "Default sync window saved.");
                    }
                    KeyCode::Char('u') if !state.minimized => {
                        state.settings.cycle_startup_panel();
                        state.save_settings(settings_path, "Startup panel saved.");
                    }
                    KeyCode::Char('v') if !state.minimized => {
                        state.settings.toggle_live_refresh();
                        last_live_poll = None;
                        state.save_settings(
                            settings_path,
                            if state.settings.live_refresh {
                                "Live refresh enabled."
                            } else {
                                "Live refresh disabled."
                            },
                        );
                    }
                    KeyCode::Char('o') if !state.minimized => {
                        state.selected = 2;
                        state.notice = connect_provider(terminal, store, Provider::OpenAi);
                        dashboard = store.dashboard()?;
                        last_live_poll = None;
                    }
                    KeyCode::Char('c') if !state.minimized => {
                        state.selected = 2;
                        state.notice = connect_provider(terminal, store, Provider::Claude);
                        dashboard = store.dashboard()?;
                        last_live_poll = None;
                    }
                    KeyCode::Char('r') if !state.minimized => {
                        state.notice = sync_connected(
                            store,
                            &dashboard,
                            state.settings.default_sync_days,
                            "Sync",
                        );
                        state.last_live_refresh = Some(Utc::now());
                        dashboard = store.dashboard()?;
                        last_live_poll = Some(Instant::now());
                    }
                    KeyCode::Char('i') if !state.minimized => state.selected = 4,
                    KeyCode::Char('e') if !state.minimized => state.selected = 5,
                    KeyCode::Char('g') if !state.minimized => state.selected = 6,
                    KeyCode::Right | KeyCode::Char('l') if !state.minimized => {
                        state.selected = (state.selected + 1) % TABS.len()
                    }
                    KeyCode::Left | KeyCode::Char('h') if !state.minimized => {
                        state.selected = state.selected.checked_sub(1).unwrap_or(TABS.len() - 1)
                    }
                    KeyCode::Char(value @ '1'..='7') if !state.minimized => {
                        state.selected = (value as usize - '1' as usize).min(TABS.len() - 1)
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn live_refresh_due(
    state: &TuiState,
    dashboard: &Dashboard,
    last_live_poll: Option<Instant>,
) -> bool {
    state.settings.live_refresh
        && !state.minimized
        && !dashboard.providers.is_empty()
        && last_live_poll
            .map(|last| last.elapsed() >= LIVE_REFRESH_INTERVAL)
            .unwrap_or(true)
}

fn connect_provider(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    store: &mut Store,
    provider: Provider,
) -> String {
    if let Err(err) = suspend_terminal(terminal) {
        return format!(
            "Could not open {} connect prompt: {err:#}",
            provider.display_name()
        );
    }

    let result = connect_provider_prompt(store, provider);

    if let Err(err) = resume_terminal(terminal) {
        return format!(
            "Could not return to Meterline after {} connect prompt: {err:#}",
            provider.display_name()
        );
    }

    match result {
        Ok(message) => message,
        Err(err) => format!("Could not connect {}: {err:#}", provider.display_name()),
    }
}

fn connect_provider_prompt(store: &mut Store, provider: Provider) -> Result<String> {
    println!("Meterline connect {}", provider.display_name());
    println!();
    println!("Meterline stores API keys in your OS keychain.");
    println!("It never asks for provider passwords or browser sessions.");
    println!("{}", provider_key_note(provider));
    println!();
    match open_provider_key_page(provider) {
        Ok(url) => println!(
            "Opened official {} key page: {url}",
            provider_product_name(provider)
        ),
        Err(err) => {
            println!("Could not open your browser automatically: {err:#}");
            println!("Open this page manually: {}", provider_key_url(provider));
        }
    }
    println!();
    println!("Next: create or copy your API key, paste it below, then press Enter.");
    println!("Leave it blank to cancel.");
    println!();

    let key = rpassword::prompt_password(format!("Paste {} API key: ", provider.display_name()))?;
    let key = key.trim();
    if key_was_cancelled(key) {
        return Ok(format!(
            "{} connection cancelled. No key was stored.",
            provider.display_name()
        ));
    }

    SecretStore::set_provider_key(provider, key)?;
    store.upsert_provider_account(provider, provider.display_name())?;
    Ok(format!(
        "{} connected. Keep using {} for live tracking.",
        provider.display_name(),
        provider_proxy_base_url(provider)
    ))
}

fn key_was_cancelled(key: &str) -> bool {
    key.trim().is_empty() || key.trim() == "\u{1b}"
}

fn sync_connected(store: &mut Store, dashboard: &Dashboard, days: i64, label: &str) -> String {
    if dashboard.providers.is_empty() {
        return "No stored API keys yet. Run `meterline connect openai` or `meterline connect claude`.".to_string();
    }

    let mut parts = Vec::new();
    for account in &dashboard.providers {
        match sync_provider(store, account.provider, days) {
            Ok(report) => parts.push(format!(
                "{}: {} usage, {} cost",
                account.provider.display_name(),
                report.usage_rows,
                report.cost_rows
            )),
            Err(err) => parts.push(format!(
                "{} skipped: {err:#}",
                account.provider.display_name()
            )),
        }
    }
    format!("{label} refreshed {}d: {}", days, parts.join(" | "))
}

pub fn render(frame: &mut Frame<'_>, dashboard: &Dashboard, selected: usize) {
    let state = TuiState {
        selected,
        ..TuiState::default()
    };
    render_app(frame, dashboard, &state, Path::new("settings.json"));
}

fn render_app(
    frame: &mut Frame<'_>,
    dashboard: &Dashboard,
    state: &TuiState,
    settings_path: &Path,
) {
    if state.minimized {
        render_minimized(frame, dashboard, state);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let palette = state.palette();
    let title =
        Paragraph::new(logo_line(dashboard, state)).block(panel_block(" Meterline ", palette));
    frame.render_widget(title, outer[0]);

    let tabs = Tabs::new(TABS.iter().copied().map(Line::from).collect::<Vec<_>>())
        .select(state.selected)
        .style(Style::default().fg(palette.soft))
        .highlight_style(
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        )
        .block(panel_block(" views ", palette));
    frame.render_widget(tabs, outer[1]);

    match state.selected {
        0 => render_home(frame, outer[2], dashboard, state),
        1 => render_models(frame, outer[2], dashboard, state),
        2 => render_providers(frame, outer[2], dashboard, palette),
        3 => render_chats(frame, outer[2], dashboard, state),
        4 => render_imports(frame, outer[2], dashboard, palette),
        5 => render_exports(frame, outer[2], palette),
        _ => render_settings(frame, outer[2], state, settings_path),
    }

    render_footer(frame, outer[3], dashboard, state);
}

fn render_minimized(frame: &mut Frame<'_>, dashboard: &Dashboard, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let palette = state.palette();
    let (label, value) = tray_value(dashboard, state);
    let line = Line::from(vec![
        Span::styled(
            "[ML]",
            Style::default()
                .fg(palette.openai)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " Meterline",
            Style::default()
                .fg(palette.claude)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(label, Style::default().fg(palette.soft)),
        Span::raw(": "),
        Span::styled(
            value,
            Style::default()
                .fg(palette.value)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(panel_block(" tray ", palette)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("q quit  m/enter restore  s hide/show values  t cycle tray metric")
            .style(Style::default().fg(palette.soft)),
        chunks[2],
    );
}

fn render_home(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(6),
        ])
        .split(area);

    render_start_here(frame, rows[0], dashboard, state);
    render_stats(frame, rows[1], dashboard, state);
    render_live_graph(frame, rows[2], dashboard, state);
}

fn render_start_here(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    let providers = connected_provider_list(dashboard);
    let title = if providers.is_empty() {
        " setup "
    } else {
        " live meter "
    };
    let mut lines = Vec::new();

    if providers.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Connect: ", Style::default().fg(palette.highlight)),
            Span::styled("[c] Claude", Style::default().fg(palette.claude)),
            Span::raw(" or "),
            Span::styled("[o] OpenAI", Style::default().fg(palette.openai)),
            Span::raw(". Paste the API key when prompted."),
        ]));
        lines.push(Line::from(
            "Then point your SDK/tool at the Meterline base URL.",
        ));
        lines.push(Line::from(
            "No browser scraping, provider passwords, or message body storage.",
        ));
    } else {
        for provider in providers {
            lines.push(Line::from(vec![
                Span::styled(
                    provider.display_name(),
                    Style::default()
                        .fg(provider_color(provider, palette))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" connected  "),
                Span::styled("base: ", Style::default().fg(palette.soft)),
                Span::raw(provider_proxy_base_url(provider)),
            ]));
        }
        lines.push(Line::from(
            "Route requests through the base URL above; usage updates here.",
        ));
        lines.push(Line::from("Add another provider from the Providers panel."));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(title, palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_stats(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    stat_card(
        frame,
        columns[0],
        " live ",
        maybe_hidden(compact_number(dashboard.live_request_count), state),
        "proxied requests",
        palette.highlight,
        palette,
    );
    stat_card(
        frame,
        columns[1],
        " tokens ",
        maybe_hidden(
            format!(
                "{} in / {} out",
                compact_number(dashboard.total_input_tokens),
                compact_number(dashboard.total_output_tokens)
            ),
            state,
        ),
        &format!("{} requests", compact_number(dashboard.total_requests)),
        palette.openai,
        palette,
    );
    stat_card(
        frame,
        columns[2],
        " last ",
        maybe_hidden(latest_activity_label(dashboard, state), state),
        "latest activity",
        palette.value,
        palette,
    );
}

fn render_live_graph(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    let lines = live_graph_lines(dashboard, state, palette);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" token graph ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn stat_card(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: String,
    foot: &str,
    accent: Color,
    palette: Palette,
) {
    let content = vec![
        Line::from(Span::styled(
            value,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            foot.to_string(),
            Style::default().fg(palette.soft),
        )),
    ];
    frame.render_widget(
        Paragraph::new(content).block(panel_block(title, palette).border_style(accent)),
        area,
    );
}

fn render_models(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    if dashboard.models.is_empty() && dashboard.hourly_usage.is_empty() {
        render_empty(
            frame,
            area,
            " models ",
            vec![
                Line::from("No model usage yet."),
                Line::from(
                    "Run `meterline daemon`, then route SDK traffic through the local base URL.",
                ),
                Line::from("OpenAI: http://127.0.0.1:37373/openai/v1"),
                Line::from("Claude: http://127.0.0.1:37373/anthropic/v1"),
            ],
            palette,
        );
        return;
    }

    let layout = if area.height >= 18 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area)
    };

    render_model_table(frame, layout[0], dashboard, state, palette);
    render_usage_rhythm(frame, layout[1], dashboard, state, palette);
}

fn render_model_table(
    frame: &mut Frame<'_>,
    area: Rect,
    dashboard: &Dashboard,
    state: &TuiState,
    palette: Palette,
) {
    if dashboard.models.is_empty() {
        render_empty(
            frame,
            area,
            " models ",
            vec![
                Line::from("No model rows yet."),
                Line::from("Live proxy requests with provider usage will fill this table."),
            ],
            palette,
        );
        return;
    }

    let rows = dashboard.models.iter().map(|model| {
        Row::new(vec![
            Cell::from(model.provider.clone()),
            Cell::from(model.model.clone()),
            Cell::from(maybe_hidden(compact_number(model.input_tokens), state)),
            Cell::from(maybe_hidden(compact_number(model.output_tokens), state)),
            Cell::from(maybe_hidden(format!("${:.4}", model.cost_usd), state)),
            Cell::from(maybe_hidden(model.imported_chats.to_string(), state)),
        ])
        .style(style_for_provider_text(&model.provider, palette))
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Percentage(34),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(["Provider", "Model", "Input", "Output", "Cost", "Activity"]).style(
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(panel_block(" models ", palette));
    frame.render_widget(table, area);
}

fn render_usage_rhythm(
    frame: &mut Frame<'_>,
    area: Rect,
    dashboard: &Dashboard,
    state: &TuiState,
    palette: Palette,
) {
    let max_rows = area.height.saturating_sub(3).max(1) as usize;
    let lines = usage_rhythm_lines(dashboard, state, palette, max_rows);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" usage rhythm by hour ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_providers(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, palette: Palette) {
    let providers = connected_provider_list(dashboard);
    if providers.is_empty() {
        render_provider_setup(frame, area, palette);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(4)])
        .split(area);

    if providers.len() == 1 || area.width < 88 {
        provider_card(frame, layout[0], dashboard, providers[0], palette);
    } else {
        let cards = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);
        for (index, provider) in providers.iter().copied().take(2).enumerate() {
            provider_card(frame, cards[index], dashboard, provider, palette);
        }
    }
    render_provider_note(frame, layout[1], dashboard, palette);
}

fn render_provider_setup(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    let lines = vec![
        Line::from(vec![
            Span::styled("[c] Claude", Style::default().fg(palette.claude)),
            Span::raw("  connect and paste API key"),
        ]),
        Line::from(vec![
            Span::styled("[o] OpenAI", Style::default().fg(palette.openai)),
            Span::raw("  add later if you need ChatGPT/API tracking"),
        ]),
        Line::from(""),
        Line::from("After connecting, set your SDK base URL to the provider's Meterline URL."),
        Line::from("Connected providers are the only ones shown on the main meter."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" provider setup ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn provider_card(
    frame: &mut Frame<'_>,
    area: Rect,
    dashboard: &Dashboard,
    provider: Provider,
    palette: Palette,
) {
    let account = provider_account(dashboard, provider);
    let accent = provider_color(provider, palette);
    let key = match provider {
        Provider::OpenAi => "[o]",
        Provider::Claude => "[c]",
    };
    let api_hint = match provider {
        Provider::OpenAi => "OpenAI live proxy + optional Usage API sync",
        Provider::Claude => "Claude live proxy + optional Usage API sync",
    };
    let status = if account.is_some() {
        Span::styled(
            "CONNECTED",
            Style::default()
                .fg(palette.value)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "NOT CONNECTED",
            Style::default()
                .fg(palette.warning)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mut lines = vec![
        Line::from(vec![Span::styled(
            provider.display_name(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::raw("Status: "), status]),
        Line::from(vec![
            Span::raw("Setup: press "),
            Span::styled(
                key,
                Style::default()
                    .fg(palette.highlight)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to open the key page, then paste the key"),
        ]),
        Line::from(format!("Data: {api_hint}")),
        Line::from(provider_key_note(provider)),
        Line::from(format!("Base: {}", provider_proxy_base_url(provider))),
    ];

    if let Some(account) = account {
        lines.push(Line::from(format!(
            "Connected: {}",
            account.connected_at.format("%Y-%m-%d %H:%M")
        )));
        lines.push(Line::from(format!(
            "Last sync: {}",
            account
                .last_synced_at
                .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "never".to_string())
        )));
    } else {
        lines.push(Line::from("Live: route SDK traffic through Meterline"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" auth ", palette).border_style(accent))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_provider_note(
    frame: &mut Frame<'_>,
    area: Rect,
    dashboard: &Dashboard,
    palette: Palette,
) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Live path: ", Style::default().fg(palette.highlight)),
            Span::raw(
                "route API calls through Meterline; only metadata and provider token counts are stored.",
            ),
        ]),
        Line::from("Keys are kept in the OS keychain. Request and response bodies are not stored."),
    ];
    let missing = missing_providers(dashboard);
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|provider| match provider {
                Provider::OpenAi => "[o] OpenAI",
                Provider::Claude => "[c] Claude",
            })
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(Line::from(format!("Add another provider: {names}")));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" live path ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_chats(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    if dashboard.recent_chats.is_empty() {
        render_empty(
            frame,
            area,
            " chats ",
            vec![
                Line::from("No imported chats yet."),
                Line::from("Optional historical metadata imports can be added later with:"),
                Line::from("meterline import chatgpt path/to/chatgpt-export.zip"),
                Line::from("meterline import claude path/to/claude-export.zip"),
            ],
            palette,
        );
        return;
    }

    let rows = dashboard.recent_chats.iter().map(|chat| {
        Row::new(vec![
            Cell::from(chat.provider.display_name()),
            Cell::from(chat.title.clone()),
            Cell::from(chat.model.clone().unwrap_or_else(|| "unknown".to_string())),
            Cell::from(maybe_hidden(
                compact_number(chat.estimated_input_tokens + chat.estimated_output_tokens),
                state,
            )),
        ])
        .style(Style::default().fg(provider_color(chat.provider, palette)))
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(["Provider", "Title", "Model", "Est tok"]).style(
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(panel_block(" recent chats ", palette));
    frame.render_widget(table, area);
}

fn render_imports(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, palette: Palette) {
    if dashboard.import_runs.is_empty() {
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    "Historical imports: ",
                    Style::default().fg(palette.highlight),
                ),
                Span::raw("optional metadata backfill, no full message bodies in v1."),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("ChatGPT", Style::default().fg(palette.openai)),
                Span::raw(": optional archive import:"),
            ]),
            Line::from("meterline import chatgpt path/to/chatgpt-export.zip"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Claude", Style::default().fg(palette.claude)),
                Span::raw(": Account settings > Export data, then:"),
            ]),
            Line::from("meterline import claude path/to/claude-export.zip"),
        ];
        render_empty(frame, area, " imports ", lines, palette);
        return;
    }

    let rows = dashboard.import_runs.iter().map(|run| {
        Row::new(vec![
            Cell::from(run.provider.as_str()),
            Cell::from(run.imported_count.to_string()),
            Cell::from(run.skipped_count.to_string()),
            Cell::from(run.ran_at.format("%Y-%m-%d %H:%M").to_string()),
            Cell::from(run.source_path.clone()),
        ])
        .style(style_for_provider_text(run.provider.as_str(), palette))
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Percentage(45),
        ],
    )
    .header(
        Row::new(["Provider", "Imported", "Skipped", "When", "Source"]).style(
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(panel_block(" imports ", palette));
    frame.render_widget(table, area);
}

fn render_exports(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    let content = vec![
        Line::from(vec![
            Span::styled("JSON: ", Style::default().fg(palette.highlight)),
            Span::raw("meterline export --format json"),
        ]),
        Line::from(vec![
            Span::styled("CSV:  ", Style::default().fg(palette.highlight)),
            Span::raw("meterline export --format csv --output meterline.csv"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Support Meterline: ", Style::default().fg(palette.claude)),
            Span::raw(crate::SUPPORT_URL),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Exports include dashboard summaries, usage buckets, cost buckets, and imported chat metadata.",
            Style::default().fg(palette.soft),
        )),
    ];
    frame.render_widget(
        Paragraph::new(content)
            .block(panel_block(" exports ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_settings(frame: &mut Frame<'_>, area: Rect, state: &TuiState, settings_path: &Path) {
    let palette = state.palette();
    let settings = &state.settings;
    let lines = vec![
        setting_line(
            "[y]",
            "Theme",
            settings.theme.to_string(),
            "balanced / openai / claude / mono",
            palette,
        ),
        setting_line(
            "[d]",
            "Sync window",
            format!("{} days", settings.default_sync_days),
            "manual sync history",
            palette,
        ),
        setting_line(
            "[u]",
            "Startup panel",
            settings.startup_panel.to_string(),
            "home / providers / chats / imports",
            palette,
        ),
        setting_line(
            "[s]",
            "Privacy",
            if settings.hide_values {
                "hidden"
            } else {
                "visible"
            }
            .to_string(),
            "default value visibility",
            palette,
        ),
        setting_line(
            "[t]",
            "Tray metric",
            settings.default_tray_metric.to_string(),
            "spend / tokens / live / sync",
            palette,
        ),
        setting_line(
            "[v]",
            "Live refresh",
            if settings.live_refresh { "on" } else { "off" }.to_string(),
            "optional API polling every 60 seconds",
            palette,
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("Saved at: ", Style::default().fg(palette.highlight)),
            Span::raw(settings_path.display().to_string()),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" settings ", palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn setting_line(
    key: &'static str,
    name: &'static str,
    value: String,
    hint: &'static str,
    palette: Palette,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            key,
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(format!("{name:<14}"), Style::default().fg(palette.soft)),
        Span::styled(value, Style::default().fg(palette.value)),
        Span::raw("  "),
        Span::styled(hint, Style::default().fg(palette.soft)),
    ])
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    let shortcuts = if dashboard.providers.is_empty() {
        Line::from(vec![
            Span::styled(
                "[c]",
                Style::default()
                    .fg(palette.claude)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Claude  "),
            Span::styled(
                "[o]",
                Style::default()
                    .fg(palette.openai)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" OpenAI  [g] settings  [q] quit"),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "[r]",
                Style::default()
                    .fg(palette.highlight)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" sync  [2] providers  [g] settings  [m] mini  [q] quit"),
        ])
    };
    let (label, value) = tray_value(dashboard, state);
    let status = Line::from(vec![
        Span::styled(
            format!("{label}: {value}  "),
            Style::default()
                .fg(palette.value)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(state.notice.clone(), Style::default().fg(palette.soft)),
    ]);
    frame.render_widget(Paragraph::new(vec![shortcuts, status]), area);
}

fn render_empty(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    palette: Palette,
) {
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(title, palette))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn panel_block(title: &str, palette: Palette) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.border))
}

fn provider_account(dashboard: &Dashboard, provider: Provider) -> Option<&ProviderAccount> {
    dashboard
        .providers
        .iter()
        .find(|account| account.provider == provider)
}

fn provider_color(provider: Provider, palette: Palette) -> Color {
    match provider {
        Provider::OpenAi => palette.openai,
        Provider::Claude => palette.claude,
    }
}

fn style_for_provider_text(value: &str, palette: Palette) -> Style {
    match value.to_ascii_lowercase().as_str() {
        "openai" | "chatgpt" => Style::default().fg(palette.openai),
        "claude" | "anthropic" => Style::default().fg(palette.claude),
        _ => Style::default(),
    }
}

fn live_graph_lines(
    dashboard: &Dashboard,
    state: &TuiState,
    palette: Palette,
) -> Vec<Line<'static>> {
    let Some(provider) = graph_provider(dashboard) else {
        return vec![
            Line::from("No provider connected."),
            Line::from("Press [c] for Claude or [o] for OpenAI."),
        ];
    };

    let mut values = [0_i64; 24];
    for row in &dashboard.hourly_usage {
        if row.provider == provider.as_str() {
            values[row.hour_utc as usize] += total_hourly_tokens(row);
        }
    }

    let total: i64 = values.iter().sum();
    let accent = provider_color(provider, palette);
    if total == 0 {
        return vec![
            Line::from(vec![
                Span::styled(
                    provider.display_name(),
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" connected"),
            ]),
            Line::from("No live usage yet. Send an API request through:"),
            Line::from(provider_proxy_base_url(provider)),
        ];
    }

    let peak = values
        .iter()
        .enumerate()
        .max_by_key(|(_, value)| *value)
        .map(|(hour, value)| (hour, *value))
        .unwrap_or((0, 0));
    let graph = if state.hide_values {
        "hidden".to_string()
    } else {
        sparkline(&values)
    };
    let total_label = maybe_hidden(format!("{} tokens", compact_number(total)), state);
    let peak_label = maybe_hidden(
        format!("{:02}:00 UTC, {}", peak.0, compact_number(peak.1)),
        state,
    );

    vec![
        Line::from(vec![
            Span::styled(
                provider.display_name(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" token graph by UTC hour"),
        ]),
        Line::from(Span::styled(graph, Style::default().fg(accent))),
        Line::from(vec![
            Span::styled("Total: ", Style::default().fg(palette.soft)),
            Span::styled(total_label, Style::default().fg(palette.value)),
            Span::raw("   "),
            Span::styled("Peak: ", Style::default().fg(palette.soft)),
            Span::styled(peak_label, Style::default().fg(palette.value)),
        ]),
    ]
}

fn sparkline(values: &[i64; 24]) -> String {
    let max = values.iter().copied().max().unwrap_or(0);
    if max <= 0 {
        return ".".repeat(24);
    }
    let levels = ['.', ':', '-', '=', '+', '*', '#', '%', '@'];
    values
        .iter()
        .map(|value| {
            if *value <= 0 {
                '.'
            } else {
                let index = ((*value * (levels.len() as i64 - 1) + max - 1) / max)
                    .clamp(1, levels.len() as i64 - 1) as usize;
                levels[index]
            }
        })
        .collect()
}

fn usage_rhythm_lines(
    dashboard: &Dashboard,
    state: &TuiState,
    palette: Palette,
    max_rows: usize,
) -> Vec<Line<'static>> {
    let claude_rows: Vec<&HourlyUsageSummary> = dashboard
        .hourly_usage
        .iter()
        .filter(|row| row.provider == "claude" && total_hourly_tokens(row) > 0)
        .collect();
    let mut rows: Vec<&HourlyUsageSummary> = if claude_rows.is_empty() {
        dashboard
            .hourly_usage
            .iter()
            .filter(|row| total_hourly_tokens(row) > 0)
            .collect()
    } else {
        claude_rows
    };

    if rows.is_empty() {
        return vec![
            Line::from("No hourly rhythm yet."),
            Line::from("Run `meterline daemon` and route SDK traffic through Meterline."),
            Line::from("Hourly rows appear when providers return usage in responses."),
        ];
    }

    rows.sort_by(|left, right| {
        total_hourly_tokens(right)
            .cmp(&total_hourly_tokens(left))
            .then_with(|| left.hour_utc.cmp(&right.hour_utc))
    });
    let max_total = rows
        .iter()
        .map(|row| total_hourly_tokens(row))
        .max()
        .unwrap_or(1);
    let row_limit = max_rows.saturating_sub(1).clamp(1, 12);
    let target = if rows.iter().all(|row| row.provider == "claude") {
        "Claude"
    } else {
        "all providers"
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("Busiest {target} hours: "),
            Style::default().fg(palette.highlight),
        ),
        Span::raw("live/API tokens by request time, shown in UTC."),
    ])];

    for row in rows.into_iter().take(row_limit) {
        let total = total_hourly_tokens(row);
        let bar = if state.hide_values {
            "hidden".to_string()
        } else {
            let width = ((total * 24 + max_total - 1) / max_total).clamp(1, 24) as usize;
            "#".repeat(width)
        };
        let value = maybe_hidden(
            format!(
                "{} tok, {} req",
                compact_number(total),
                compact_number(row.requests)
            ),
            state,
        );
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:02}:00 UTC ", row.hour_utc),
                Style::default().fg(palette.soft),
            ),
            Span::styled(
                format!("{:<7}", provider_name_from_str(&row.provider)),
                style_for_provider_text(&row.provider, palette).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:<24} ", bar),
                style_for_provider_text(&row.provider, palette),
            ),
            Span::styled(value, Style::default().fg(palette.value)),
        ]));
    }
    lines
}

fn total_hourly_tokens(row: &HourlyUsageSummary) -> i64 {
    row.input_tokens + row.output_tokens
}

fn provider_name_from_str(value: &str) -> &'static str {
    match value {
        "openai" | "chatgpt" => "ChatGPT",
        "claude" | "anthropic" => "Claude",
        _ => "Other",
    }
}

fn connected_provider_list(dashboard: &Dashboard) -> Vec<Provider> {
    [Provider::Claude, Provider::OpenAi]
        .into_iter()
        .filter(|provider| provider_account(dashboard, *provider).is_some())
        .collect()
}

fn missing_providers(dashboard: &Dashboard) -> Vec<Provider> {
    [Provider::Claude, Provider::OpenAi]
        .into_iter()
        .filter(|provider| provider_account(dashboard, *provider).is_none())
        .collect()
}

fn graph_provider(dashboard: &Dashboard) -> Option<Provider> {
    let providers = connected_provider_list(dashboard);
    if providers.len() == 1 {
        return providers.first().copied();
    }

    providers.into_iter().max_by_key(|provider| {
        dashboard
            .hourly_usage
            .iter()
            .filter(|row| row.provider == provider.as_str())
            .map(total_hourly_tokens)
            .sum::<i64>()
    })
}

fn compact_number(value: i64) -> String {
    let abs = value.abs();
    if abs >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn logo_line<'a>(dashboard: &Dashboard, state: &TuiState) -> Line<'a> {
    let palette = state.palette();
    let (label, value) = tray_value(dashboard, state);
    let live = if state.settings.live_refresh {
        "live on"
    } else {
        "live off"
    };
    Line::from(vec![
        Span::styled(
            "[ML]",
            Style::default()
                .fg(palette.openai)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " Meter",
            Style::default()
                .fg(palette.openai)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "line",
            Style::default()
                .fg(palette.claude)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "local live AI usage meter",
            Style::default().fg(palette.soft),
        ),
        Span::raw("  |  "),
        Span::styled(live, Style::default().fg(palette.highlight)),
        Span::raw("  |  "),
        Span::styled(label, Style::default().fg(palette.soft)),
        Span::raw(": "),
        Span::styled(
            value,
            Style::default()
                .fg(palette.value)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn tray_value(dashboard: &Dashboard, state: &TuiState) -> (&'static str, String) {
    let value = match state.tray_item {
        0 => format!("${:.4}", dashboard.total_cost_usd),
        1 => format!(
            "{} total",
            compact_number(dashboard.total_input_tokens + dashboard.total_output_tokens)
        ),
        2 => dashboard.live_request_count.to_string(),
        3 => latest_sync_label(dashboard, state),
        _ => String::new(),
    };

    (
        match state.tray_item {
            0 => "Spend",
            1 => "Tokens",
            2 => "Live",
            3 => "Sync",
            _ => "Metric",
        },
        maybe_hidden(value, state),
    )
}

fn latest_activity_label(dashboard: &Dashboard, _state: &TuiState) -> String {
    dashboard
        .recent_live_requests
        .first()
        .map(|request| {
            format!(
                "{} {}",
                request.provider.display_name(),
                request.started_at.format("%H:%M:%S")
            )
        })
        .unwrap_or_else(|| "waiting".to_string())
}

fn latest_sync_label(dashboard: &Dashboard, _state: &TuiState) -> String {
    dashboard
        .providers
        .iter()
        .filter_map(|provider| {
            provider
                .last_synced_at
                .map(|synced| (provider.provider.display_name(), synced))
        })
        .max_by_key(|(_, synced)| synced.timestamp())
        .map(|(name, synced)| format!("{name} {}", synced.format("%b %d %H:%M")))
        .unwrap_or_else(|| "never".to_string())
}

fn maybe_hidden(value: String, state: &TuiState) -> String {
    if state.hide_values {
        "hidden".to_string()
    } else {
        value
    }
}

fn palette_for(theme: Theme) -> Palette {
    match theme {
        Theme::Balanced => Palette {
            openai: Color::Rgb(16, 163, 127),
            claude: Color::Rgb(204, 120, 55),
            highlight: Color::Rgb(103, 232, 249),
            value: Color::Rgb(134, 239, 172),
            border: Color::Rgb(71, 85, 105),
            soft: Color::Rgb(148, 163, 184),
            warning: Color::Rgb(251, 191, 36),
        },
        Theme::OpenAi => Palette {
            openai: Color::Rgb(16, 163, 127),
            claude: Color::Rgb(180, 122, 75),
            highlight: Color::Rgb(52, 211, 153),
            value: Color::Rgb(134, 239, 172),
            border: Color::Rgb(22, 101, 52),
            soft: Color::Rgb(148, 163, 184),
            warning: Color::Rgb(251, 191, 36),
        },
        Theme::Claude => Palette {
            openai: Color::Rgb(72, 187, 150),
            claude: Color::Rgb(204, 120, 55),
            highlight: Color::Rgb(251, 191, 36),
            value: Color::Rgb(253, 186, 116),
            border: Color::Rgb(120, 72, 35),
            soft: Color::Rgb(161, 161, 170),
            warning: Color::Rgb(250, 204, 21),
        },
        Theme::Mono => Palette {
            openai: Color::White,
            claude: Color::White,
            highlight: Color::White,
            value: Color::White,
            border: Color::DarkGray,
            soft: Color::Gray,
            warning: Color::White,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_empty_dashboard_with_auth_guidance() {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &Dashboard::default(), 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = format!("{buffer:?}");
        assert!(text.contains("Meterline"));
        assert!(text.contains("OpenAI"));
        assert!(text.contains("Claude"));
        assert!(text.contains("Connect"));
        assert!(text.contains("Settings"));
    }

    #[test]
    fn connected_claude_home_does_not_show_openai_setup() {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).unwrap();
        let dashboard = Dashboard {
            providers: vec![ProviderAccount {
                provider: Provider::Claude,
                label: "Claude".to_string(),
                connected_at: Utc::now(),
                last_synced_at: None,
            }],
            ..Dashboard::default()
        };
        terminal.draw(|frame| render(frame, &dashboard, 0)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = format!("{buffer:?}");
        assert!(text.contains("Claude"));
        assert!(!text.contains("OpenAI base"));
        assert!(!text.contains("OpenAI key"));
    }

    #[test]
    fn renders_settings_panel() {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &Dashboard::default(), 6))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = format!("{buffer:?}");
        assert!(text.contains("Theme"));
        assert!(text.contains("Live refresh"));
    }

    #[test]
    fn renders_usage_rhythm_panel() {
        let backend = TestBackend::new(120, 34);
        let mut terminal = Terminal::new(backend).unwrap();
        let dashboard = Dashboard {
            hourly_usage: vec![HourlyUsageSummary {
                provider: "claude".to_string(),
                hour_utc: 14,
                input_tokens: 120,
                output_tokens: 80,
                imported_chats: 2,
                ..HourlyUsageSummary::default()
            }],
            ..Dashboard::default()
        };
        terminal.draw(|frame| render(frame, &dashboard, 1)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = format!("{buffer:?}");
        assert!(text.contains("usage rhythm"));
        assert!(text.contains("14:00 UTC"));
        assert!(text.contains("Claude"));
    }
}
