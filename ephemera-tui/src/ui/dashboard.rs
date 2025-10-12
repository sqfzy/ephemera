use crate::app::{App, ConnectionStatus};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Sparkline},
};
use rust_decimal::prelude::ToPrimitive;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Symbol cards
            Constraint::Length(8),  // System status
            Constraint::Min(0),     // Recent activity
        ])
        .split(area);

    render_symbol_cards(f, app, chunks[0]);
    render_system_status(f, app, chunks[1]);
    render_recent_activity(f, app, chunks[2]);
}

fn render_symbol_cards(f: &mut Frame, app: &App, area: Rect) {
    let num_symbols = app.symbols.len();
    let constraints: Vec<Constraint> = (0..num_symbols)
        .map(|_| Constraint::Percentage((100 / num_symbols) as u16))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, symbol) in app.symbols.iter().enumerate() {
        render_symbol_card(f, app, symbol, chunks[i], i, i == app.selected_symbol_index);
    }
}

fn render_symbol_card(
    f: &mut Frame,
    app: &App,
    symbol: &str,
    area: Rect,
    index: usize,
    is_selected: bool,
) {
    let stats = app.trade_stats.get(symbol);

    let (price, volume, change) = if let Some(s) = stats {
        (s.last_price, s.volume, s.price_change_1m)
    } else {
        (0.0, 0.0, 0.0)
    };

    let (change_icon, change_color) = if change > 0.0 {
        ("📈", Color::Green)
    } else if change < 0.0 {
        ("📉", Color::Red)
    } else {
        ("➖", Color::Gray)
    };

    // Get recent prices for sparkline
    let prices: Vec<u64> = app
        .recent_trades
        .iter()
        .filter(|t| t.symbol == symbol)
        .take(20)
        .filter_map(|t| t.price.to_f64().map(|p| p as u64))
        .collect();

    let lines = vec![
        Line::from(vec![Span::styled(
            symbol,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("${:.2}", price),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            format!("{} {:.2}%", change_icon, change.abs()),
            Style::default().fg(change_color),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("Vol: {:.2}", volume),
            Style::default().fg(Color::Gray),
        )]),
    ];

    let border_style = if is_selected {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let title = if is_selected {
        format!("{} [{}]", symbol, index + 1)
    } else {
        format!("{} ({})", symbol, index + 1)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area for text and sparkline
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner_chunks[0]);

    // Render sparkline if we have data
    if !prices.is_empty() {
        let sparkline = Sparkline::default()
            .data(&prices)
            .style(Style::default().fg(change_color));
        f.render_widget(sparkline, inner_chunks[1]);
    }
}

fn render_system_status(f: &mut Frame, app: &App, area: Rect) {
    let (conn_status, conn_color) = match app.system_stats.connection_status {
        ConnectionStatus::Connected => ("Connected", Color::Green),
        ConnectionStatus::Disconnected => ("Disconnected", Color::Red),
        ConnectionStatus::Reconnecting => ("Reconnecting", Color::Yellow),
    };

    let lines = vec![
        Line::from(vec![Span::styled(
            "System Status",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::raw("🔌 OKX WebSocket:  "),
            Span::styled(conn_status, Style::default().fg(conn_color)),
        ]),
        Line::from(vec![Span::raw(format!(
            "📊 Trades/sec:     {:.1}  (Peak: {:.1})",
            app.system_stats.trades_per_sec, app.system_stats.peak_trades_per_sec
        ))]),
        Line::from(vec![Span::raw(format!(
            "📈 Total Trades:   {}",
            app.trade_count
        ))]),
        Line::from(vec![Span::raw(format!(
            "🕯️  Total Candles:  {}",
            app.candle_count
        ))]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray))
        .title("System Status");

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn render_recent_activity(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Recent Activity",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    // Show last 5 significant trades (volume > threshold)
    let threshold = 1.0; // Adjust based on your needs
    let significant_trades: Vec<_> = app
        .recent_trades
        .iter()
        .filter(|t| t.quantity.to_f64().unwrap_or(0.0) > threshold)
        .take(5)
        .collect();

    if significant_trades.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No significant activity yet...",
            Style::default().fg(Color::Gray),
        )]));
    } else {
        for trade in significant_trades {
            let price = trade.price.to_f64().unwrap_or(0.0);
            let qty = trade.quantity.to_f64().unwrap_or(0.0);
            let notional = price * qty;

            lines.push(Line::from(vec![
                Span::raw("• "),
                Span::styled(
                    format!("{}", trade.symbol),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(format!(" {:.2} @ ${:.2} (${:.0})", qty, price, notional)),
            ]));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray))
        .title("Recent Activity (Volume > 1.0)");

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
