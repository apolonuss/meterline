use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode};
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

use crate::models::{Dashboard, Provider, ProviderAccount};
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
            notice: "Live refresh is on. Start with [o] OpenAI, [c] Claude, or import official export zips.".to_string(),
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

    loop {
        if live_refresh_due(&state, &dashboard, last_live_poll) {
            state.notice = "Live refresh running from official provider APIs...".to_string();
            terminal.draw(|frame| render_app(frame, &dashboard, &state, settings_path))?;
            state.notice = sync_connected(store, &dashboard, 1, "Live");
            state.last_live_refresh = Some(Utc::now());
            dashboard = store.dashboard()?;
            last_live_poll = Some(Instant::now());
        }

        terminal.draw(|frame| render_app(frame, &dashboard, &state, settings_path))?;
        if event::poll(Duration::from_millis(160))? {
            if let Event::Key(key) = event::read()? {
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
                        state.notice = handle_connect(terminal, store, Provider::OpenAi);
                        dashboard = store.dashboard()?;
                        last_live_poll = None;
                    }
                    KeyCode::Char('c') if !state.minimized => {
                        state.selected = 2;
                        state.notice = handle_connect(terminal, store, Provider::Claude);
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

fn handle_connect(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    store: &mut Store,
    provider: Provider,
) -> String {
    match connect_provider(terminal, store, provider) {
        Ok(message) => message,
        Err(err) => format!("Could not connect {}: {err:#}", provider.display_name()),
    }
}

fn connect_provider(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    store: &mut Store,
    provider: Provider,
) -> Result<String> {
    suspend_terminal(terminal)?;
    let result = connect_provider_prompt(store, provider);

    println!();
    match &result {
        Ok(message) => println!("{message}"),
        Err(err) => eprintln!("Could not connect {}: {err:#}", provider.display_name()),
    }
    println!("Press Enter to return to Meterline.");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    resume_terminal(terminal)?;
    result
}

fn connect_provider_prompt(store: &mut Store, provider: Provider) -> Result<String> {
    println!("Meterline connect {}", provider.display_name());
    println!();
    println!("This stores an API/Admin key in your OS keychain.");
    println!("Meterline never asks for provider passwords or browser sessions.");
    println!();

    let hint = match provider {
        Provider::OpenAi => "OpenAI API key for organization usage/cost endpoints",
        Provider::Claude => "Anthropic Admin API key for Usage & Cost API",
    };
    let key = rpassword::prompt_password(format!("Paste {hint}: "))?;
    let key = key.trim();
    if key.is_empty() {
        bail!("empty key");
    }

    SecretStore::set_provider_key(provider, key)?;
    store.upsert_provider_account(provider, provider.display_name())?;
    Ok(format!(
        "{} connected. Live refresh will update after auth; press [r] for a full sync.",
        provider.display_name()
    ))
}

fn sync_connected(store: &mut Store, dashboard: &Dashboard, days: i64, label: &str) -> String {
    if dashboard.providers.is_empty() {
        return "Nothing to sync yet. Press [o] OpenAI or [c] Claude to connect first.".to_string();
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
    let rows = if has_any_data(dashboard) {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(8)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(11), Constraint::Min(7)])
            .split(area)
    };

    render_start_here(frame, rows[0], dashboard, state);
    render_stats(frame, rows[1], dashboard, state);
}

fn render_start_here(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let palette = state.palette();
    let title = if has_any_data(dashboard) {
        " next actions "
    } else {
        " start here "
    };
    let live_label = if state.settings.live_refresh {
        "on"
    } else {
        "off"
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Connect auth: ", Style::default().fg(palette.highlight)),
        Span::styled("[o] OpenAI", Style::default().fg(palette.openai)),
        Span::raw(" API key   "),
        Span::styled("[c] Claude", Style::default().fg(palette.claude)),
        Span::raw(" Admin key   "),
        Span::styled("[r] Sync", Style::default().fg(palette.highlight)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Live data: ", Style::default().fg(palette.highlight)),
        Span::raw(format!(
            "official authenticated polling every 60s ({live_label}); usage freshness depends on provider reporting"
        )),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Import chats: ", Style::default().fg(palette.highlight)),
        Span::raw("official export zips only, metadata-first, no message bodies in v1"),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(
        "meterline import chatgpt path/to/chatgpt-export.zip",
    ));
    lines.push(Line::from(
        "meterline import claude path/to/claude-export.zip",
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "No provider passwords. No web scraping. No usage webhooks are required for v1.",
        Style::default().fg(palette.soft),
    )));

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
        " spend ",
        maybe_hidden(format!("${:.4}", dashboard.total_cost_usd), state),
        "official cost APIs",
        palette.claude,
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
        " freshness ",
        maybe_hidden(latest_refresh_label(dashboard, state), state),
        "live/manual sync status",
        palette.highlight,
        palette,
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
    if dashboard.models.is_empty() {
        render_empty(
            frame,
            area,
            " models ",
            vec![
                Line::from("No model usage yet."),
                Line::from("Press [o] or [c] to connect a provider, then press [r] to sync."),
                Line::from("Live refresh updates connected provider data every 60 seconds."),
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
        Row::new(["Provider", "Model", "Input", "Output", "Cost", "Chats"]).style(
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(panel_block(" models ", palette));
    frame.render_widget(table, area);
}

fn render_providers(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, palette: Palette) {
    let layout = if area.width < 88 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Min(4),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(4)])
            .split(area)
    };

    if area.width < 88 {
        provider_card(frame, layout[0], dashboard, Provider::OpenAi, palette);
        provider_card(frame, layout[1], dashboard, Provider::Claude, palette);
        render_provider_note(frame, layout[2], palette);
    } else {
        let cards = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);
        provider_card(frame, cards[0], dashboard, Provider::OpenAi, palette);
        provider_card(frame, cards[1], dashboard, Provider::Claude, palette);
        render_provider_note(frame, layout[1], palette);
    }
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
        Provider::OpenAi => "OpenAI usage/cost API",
        Provider::Claude => "Anthropic Usage & Cost Admin API",
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
            Span::raw("Auth: press "),
            Span::styled(
                key,
                Style::default()
                    .fg(palette.highlight)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to paste key"),
        ]),
        Line::from(format!("Data: {api_hint}")),
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
        lines.push(Line::from("Sync: connect first, then live refresh starts"));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" auth ", palette).border_style(accent))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_provider_note(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Live data: ", Style::default().fg(palette.highlight)),
            Span::raw(
                "Meterline polls official authenticated usage APIs once per minute when enabled.",
            ),
        ]),
        Line::from(
            "Use provider API/Admin keys for API usage. Use official data exports for ChatGPT/Claude chat metadata.",
        ),
        Line::from("Provider passwords are never requested or stored."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" official paths ", palette))
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
                Line::from(
                    "Export your ChatGPT or Claude data from the official product, then import the zip:",
                ),
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
                    "Import official exports: ",
                    Style::default().fg(palette.highlight),
                ),
                Span::raw("chat metadata first, no full message bodies in v1."),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("ChatGPT", Style::default().fg(palette.openai)),
                Span::raw(": Settings > Data Controls > Export data, then:"),
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
            "spend / tokens / chats / sync",
            palette,
        ),
        setting_line(
            "[v]",
            "Live refresh",
            if settings.live_refresh { "on" } else { "off" }.to_string(),
            "official polling every 60 seconds",
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
    let shortcuts = Line::from(vec![
        Span::styled(
            "[o]",
            Style::default()
                .fg(palette.openai)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" OpenAI  "),
        Span::styled(
            "[c]",
            Style::default()
                .fg(palette.claude)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Claude  "),
        Span::styled(
            "[r]",
            Style::default()
                .fg(palette.highlight)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" sync  [v] live  [g] settings  [m] mini  [q] quit"),
    ]);
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

fn has_any_data(dashboard: &Dashboard) -> bool {
    !dashboard.providers.is_empty()
        || !dashboard.models.is_empty()
        || !dashboard.recent_chats.is_empty()
        || !dashboard.import_runs.is_empty()
        || dashboard.total_requests > 0
        || dashboard.imported_chats > 0
        || dashboard.total_cost_usd > 0.0
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
            "official AI usage console",
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
        2 => dashboard.imported_chats.to_string(),
        3 => latest_sync_label(dashboard, state),
        _ => String::new(),
    };

    (
        match state.tray_item {
            0 => "Spend",
            1 => "Tokens",
            2 => "Chats",
            3 => "Sync",
            _ => "Metric",
        },
        maybe_hidden(value, state),
    )
}

fn latest_refresh_label(dashboard: &Dashboard, state: &TuiState) -> String {
    if let Some(value) = state.last_live_refresh {
        return format!("live {}", value.format("%H:%M:%S"));
    }
    latest_sync_label(dashboard, state)
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
        assert!(text.contains("Connect auth"));
        assert!(text.contains("Settings"));
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
}
