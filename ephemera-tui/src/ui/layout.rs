use super::{candles, dashboard, help, logs, orderbook, performance, trades};
use crate::app::{AlertSeverity, App, ConnectionStatus, Tab};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub fn render(f: &mut Frame, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Status bar
            Constraint::Length(3), // Tabs
            Constraint::Min(0),    // Main content
            Constraint::Length(if app.show_alerts && !app.alerts.is_empty() {
                5
            } else {
                0
            }), // Alerts
        ])
        .split(f.area());

    render_status_bar(f, app, main_chunks[0]);
    render_tabs(f, app, main_chunks[1]);
    render_main_content(f, app, main_chunks[2]);

    if app.show_alerts && !app.alerts.is_empty() {
        render_alerts(f, app, main_chunks[3]);
    }
}

fn render_status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let now = chrono::Utc::now();
    let uptime = now.signed_duration_since(app.start_time);

    let (conn_icon, conn_color) = match app.system_stats.connection_status {
        ConnectionStatus::Connected => ("🟢", Color::Green),
        ConnectionStatus::Disconnected => ("🔴", Color::Red),
        ConnectionStatus::Reconnecting => ("🟡", Color::Yellow),
    };

    let status = Line::from(vec![
        Span::styled(
            "Ephemera TUI",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{} OKX", conn_icon),
            Style::default().fg(conn_color),
        ),
        Span::raw(" | "),
        Span::raw(format!("⏱️  {}s", uptime.num_seconds())),
        Span::raw(" | "),
        Span::raw(format!("📊 {:.0} t/s", app.system_stats.trades_per_sec)),
        Span::raw(" | "),
        Span::raw(format!("⚡ {:.1}ms", app.system_stats.avg_latency_ms)),
        Span::raw(" | "),
        Span::raw(now.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        Span::raw(" | "),
        Span::styled(
            if app.paused { "⏸️  PAUSED" } else { "" },
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let paragraph = Paragraph::new(status).block(block);
    f.render_widget(paragraph, area);
}

fn render_tabs(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let titles = vec![
        Tab::Dashboard.title(),
        Tab::Trades.title(),
        Tab::Candles.title(),
        Tab::OrderBook.title(),
        Tab::Performance.title(),
        Tab::Logs.title(),
        Tab::Help.title(),
    ];

    let selected = match app.current_tab {
        Tab::Dashboard => 0,
        Tab::Trades => 1,
        Tab::Candles => 2,
        Tab::OrderBook => 3,
        Tab::Performance => 4,
        Tab::Logs => 5,
        Tab::Help => 6,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Navigation (Tab | d/t/c/o/p/l/h | Space=pause | a=alerts)"),
        )
        .select(selected)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

fn render_main_content(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    match app.current_tab {
        Tab::Dashboard => dashboard::render(f, app, area),
        Tab::Trades => trades::render(f, app, area),
        Tab::Candles => candles::render(f, app, area),
        Tab::OrderBook => orderbook::render(f, app, area),
        Tab::Performance => performance::render(f, app, area),
        Tab::Logs => logs::render(f, app, area),
        Tab::Help => help::render(f, app, area),
    }
}

fn render_alerts(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let recent_alerts: Vec<_> = app.alerts.iter().take(3).collect();

    let lines: Vec<Line> = recent_alerts
        .iter()
        .map(|alert| {
            let (icon, color) = match alert.severity {
                AlertSeverity::Info => ("ℹ️ ", Color::Cyan),
                AlertSeverity::Warning => ("⚠️ ", Color::Yellow),
                AlertSeverity::Critical => ("🔴", Color::Red),
            };

            let time = alert.timestamp.format("%H:%M:%S");
            Line::from(vec![
                Span::styled(icon.to_string(), Style::default().fg(color)),
                Span::styled(format!("[{}]", time), Style::default().fg(Color::Gray)),
                Span::raw(format!(" {}", alert.message)),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title("⚡ Recent Alerts (Press 'a' to toggle)"),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}
