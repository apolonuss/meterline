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
    let mut selected = 0usize;
    loop {
        terminal.draw(|frame| render(frame, &dashboard, selected))?;
        if event::poll(Duration::from_millis(160))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Right | KeyCode::Char('l') => selected = (selected + 1) % TABS.len(),
                    KeyCode::Left | KeyCode::Char('h') => {
                        selected = selected.checked_sub(1).unwrap_or(TABS.len() - 1)
                    }
                    KeyCode::Char(value @ '1'..='6') => {
                        selected = (value as usize - '1' as usize).min(TABS.len() - 1)
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn render(frame: &mut Frame<'_>, dashboard: &Dashboard, selected: usize) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "Meterline",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  usage, costs, models, chats"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, outer[0]);

    let tabs = Tabs::new(TABS.iter().copied().map(Line::from).collect::<Vec<_>>())
        .select(selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(tabs, outer[1]);

    match selected {
        0 => render_overview(frame, outer[2], dashboard),
        1 => render_models(frame, outer[2], dashboard),
        2 => render_providers(frame, outer[2], dashboard),
        3 => render_chats(frame, outer[2], dashboard),
        4 => render_imports(frame, outer[2], dashboard),
        _ => render_exports(frame, outer[2]),
    }

    let help = Paragraph::new("q quit  left/right or h/l switch panels  1-6 jump");
    frame.render_widget(help, outer[3]);
}

fn render_overview(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard) {
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
        format!("${:.4}", dashboard.total_cost_usd),
        "from provider cost APIs",
    );
    stat_card(
        frame,
        columns[1],
        "Tokens",
        format!(
            "{} in / {} out",
            compact_number(dashboard.total_input_tokens),
            compact_number(dashboard.total_output_tokens)
        ),
        &format!("{} requests", compact_number(dashboard.total_requests)),
    );
    stat_card(
        frame,
        columns[2],
        "Chats",
        dashboard.imported_chats.to_string(),
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

fn render_models(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard) {
    let rows = dashboard.models.iter().map(|model| {
        Row::new(vec![
            Cell::from(model.provider.clone()),
            Cell::from(model.model.clone()),
            Cell::from(compact_number(model.input_tokens)),
            Cell::from(compact_number(model.output_tokens)),
            Cell::from(format!("${:.4}", model.cost_usd)),
            Cell::from(model.imported_chats.to_string()),
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

fn render_chats(frame: &mut Frame<'_>, area: Rect, dashboard: &Dashboard) {
    let rows = dashboard.recent_chats.iter().map(|chat| {
        Row::new(vec![
            Cell::from(chat.provider.display_name()),
            Cell::from(chat.title.clone()),
            Cell::from(chat.model.clone().unwrap_or_else(|| "unknown".to_string())),
            Cell::from(compact_number(
                chat.estimated_input_tokens + chat.estimated_output_tokens,
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
