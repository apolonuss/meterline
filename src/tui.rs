use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs};
use ratatui::{Frame, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;

use crate::models::Dashboard;
use crate::store::Store;

const TABS: [&str; 6] = [
    "Overview",
    "Models",
    "Providers",
    "Chats",
    "Imports",
    "Exports",
];
const TRAY_ITEMS: [&str; 4] = ["Spend", "Tokens", "Chats", "Sync"];

#[derive(Clone, Debug, Default)]
struct TuiState {
    selected: usize,
    tray_item: usize,
    minimized: bool,
    hide_values: bool,
}

pub fn run(store: &Store) -> Result<()> {
    let dashboard = store.dashboard()?;
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, dashboard);
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

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, dashboard: Dashboard) -> Result<()> {
    let mut state = TuiState::default();
    loop {
        terminal.draw(|frame| render_app(frame, &dashboard, &state))?;
        if event::poll(Duration::from_millis(160))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('m') => state.minimized = !state.minimized,
                    KeyCode::Enter if state.minimized => state.minimized = false,
                    KeyCode::Char('s') => state.hide_values = !state.hide_values,
                    KeyCode::Char('t') | KeyCode::Tab => {
                        state.tray_item = (state.tray_item + 1) % TRAY_ITEMS.len()
                    }
                    KeyCode::Right | KeyCode::Char('l') if !state.minimized => {
                        state.selected = (state.selected + 1) % TABS.len()
                    }
                    KeyCode::Left | KeyCode::Char('h') if !state.minimized => {
                        state.selected = state.selected.checked_sub(1).unwrap_or(TABS.len() - 1)
                    }
                    KeyCode::Char(value @ '1'..='6') if !state.minimized => {
                        state.selected = (value as usize - '1' as usize).min(TABS.len() - 1)
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn render(frame: &mut Frame<'_>, dashboard: &Dashboard, selected: usize) {
    let state = TuiState {
        selected,
        ..TuiState::default()
    };
    render_app(frame, dashboard, &state);
}

fn render_app(frame: &mut Frame<'_>, dashboard: &Dashboard, state: &TuiState) {
    if state.minimized {
        render_minimized(frame, dashboard, state);
        return;
    }

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let title =
        Paragraph::new(logo_line(dashboard, state)).block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, outer[0]);

    let tabs = Tabs::new(TABS.iter().copied().map(Line::from).collect::<Vec<_>>())
        .select(state.selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(tabs, outer[1]);

    match state.selected {
        0 => render_overview(frame, outer[2], dashboard, state),
        1 => render_models(frame, outer[2], dashboard, state),
        2 => render_providers(frame, outer[2], dashboard),
        3 => render_chats(frame, outer[2], dashboard, state),
        4 => render_imports(frame, outer[2], dashboard),
        _ => render_exports(frame, outer[2]),
    }

    let help = Paragraph::new(format!(
        "q quit  m minimize  s {} values  t tray: {}  left/right or h/l switch panels  1-6 jump",
        if state.hide_values { "show" } else { "hide" },
        tray_value(dashboard, state).1
    ));
    frame.render_widget(help, outer[3]);
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

    let (label, value) = tray_value(dashboard, state);
    let line = Line::from(vec![
        Span::styled(
            "[ML] Meterline",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(label, Style::default().fg(Color::Gray)),
        Span::raw(": "),
        Span::styled(
            value,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("q quit  m/enter restore  s hide/show values  t cycle tray metric"),
        chunks[2],
    );
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
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
        "Spend",
        maybe_hidden(format!("${:.4}", dashboard.total_cost_usd), state),
        "from provider cost APIs",
    );
    stat_card(
        frame,
        columns[1],
        "Tokens",
        maybe_hidden(
            format!(
                "{} in / {} out",
                compact_number(dashboard.total_input_tokens),
                compact_number(dashboard.total_output_tokens)
            ),
            state,
        ),
        &format!("{} requests", compact_number(dashboard.total_requests)),
    );
    stat_card(
        frame,
        columns[2],
        "Chats",
        maybe_hidden(dashboard.imported_chats.to_string(), state),
        "metadata imported locally",
    );
}

fn stat_card(frame: &mut Frame<'_>, area: Rect, title: &str, value: String, foot: &str) {
    let content = vec![
        Line::from(Span::styled(
            value,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            foot.to_string(),
            Style::default().fg(Color::Gray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(content).block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn render_models(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
    let rows = dashboard.models.iter().map(|model| {
        Row::new(vec![
            Cell::from(model.provider.clone()),
            Cell::from(model.model.clone()),
            Cell::from(maybe_hidden(compact_number(model.input_tokens), state)),
            Cell::from(maybe_hidden(compact_number(model.output_tokens), state)),
            Cell::from(maybe_hidden(format!("${:.4}", model.cost_usd), state)),
            Cell::from(maybe_hidden(model.imported_chats.to_string(), state)),
        ])
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
        Row::new(["Provider", "Model", "Input", "Output", "Cost", "Chats"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(Block::default().title("Models").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_providers(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard) {
    let rows = dashboard.providers.iter().map(|provider| {
        Row::new(vec![
            Cell::from(provider.provider.display_name()),
            Cell::from(provider.label.clone()),
            Cell::from(provider.connected_at.format("%Y-%m-%d %H:%M").to_string()),
            Cell::from(
                provider
                    .last_synced_at
                    .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "never".to_string()),
            ),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(34),
            Constraint::Length(18),
            Constraint::Length(18),
        ],
    )
    .header(
        Row::new(["Provider", "Label", "Connected", "Last sync"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(Block::default().title("Providers").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_chats(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard, state: &TuiState) {
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
        Row::new(["Provider", "Title", "Model", "Est tok"]).style(Style::default().fg(Color::Cyan)),
    )
    .block(Block::default().title("Recent chats").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_imports(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard) {
    let rows = dashboard.import_runs.iter().map(|run| {
        Row::new(vec![
            Cell::from(run.provider.as_str()),
            Cell::from(run.imported_count.to_string()),
            Cell::from(run.skipped_count.to_string()),
            Cell::from(run.ran_at.format("%Y-%m-%d %H:%M").to_string()),
            Cell::from(run.source_path.clone()),
        ])
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
        Row::new(["Provider", "Imported", "Skipped", "When", "Source"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .block(Block::default().title("Imports").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_exports(frame: &mut Frame<'_>, area: Rect) {
    let content = vec![
        Line::from("meterline export --format json"),
        Line::from("meterline export --format csv --output meterline.csv"),
        Line::from(""),
        Line::from(format!("Support Meterline: {}", crate::SUPPORT_URL)),
        Line::from(""),
        Line::from(Span::styled(
            "Exports include dashboard summaries, usage buckets, cost buckets, and imported chat metadata.",
            Style::default().fg(Color::Gray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(content).block(Block::default().title("Exports").borders(Borders::ALL)),
        area,
    );
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
    let (label, value) = tray_value(dashboard, state);
    Line::from(vec![
        Span::styled(
            "[ML]",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Meterline  "),
        Span::styled(
            "usage, costs, models, chats",
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  |  "),
        Span::styled(label, Style::default().fg(Color::Gray)),
        Span::raw(": "),
        Span::styled(
            value,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn tray_value(dashboard: &Dashboard, state: &TuiState) -> (&'static str, String) {
    let value = match TRAY_ITEMS[state.tray_item] {
        "Spend" => format!("${:.4}", dashboard.total_cost_usd),
        "Tokens" => format!(
            "{} total",
            compact_number(dashboard.total_input_tokens + dashboard.total_output_tokens)
        ),
        "Chats" => dashboard.imported_chats.to_string(),
        "Sync" => latest_sync_label(dashboard),
        _ => String::new(),
    };

    (TRAY_ITEMS[state.tray_item], maybe_hidden(value, state))
}

fn latest_sync_label(dashboard: &Dashboard) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_empty_dashboard() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, &Dashboard::default(), 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = format!("{buffer:?}");
        assert!(text.contains("Meterline"));
    }
}
