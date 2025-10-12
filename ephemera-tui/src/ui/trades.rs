use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
};
use rust_decimal::prelude::ToPrimitive;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[0]);

    render_trade_list(f, app, main_chunks[0]);
    render_trade_detail(f, app, main_chunks[1]);
    render_stats(f, app, chunks[1]);
}

fn render_trade_list(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Symbol", "Price", "Quantity", "Time"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .recent_trades
        .iter()
        .enumerate()
        .map(|(i, trade)| {
            let time = chrono::DateTime::from_timestamp_millis(trade.timestamp_ms as i64)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "N/A".to_string());

            let style = if i == app.selected_trade_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                trade.symbol.to_string(),
                format!("{:.2}", trade.price),
                format!("{:.4}", trade.quantity),
                time,
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Trade List (↑/↓ or j/k to select)"),
    );

    f.render_widget(table, area);
}

fn render_trade_detail(f: &mut Frame, app: &App, area: Rect) {
    let lines = if let Some(trade) = app.get_selected_trade() {
        let price = trade.price.to_f64().unwrap_or(0.0);
        let qty = trade.quantity.to_f64().unwrap_or(0.0);
        let notional = price * qty;

        let time = chrono::DateTime::from_timestamp_millis(trade.timestamp_ms as i64)
            .map(|dt| dt.format("%H:%M:%S%.3f").to_string())
            .unwrap_or_else(|| "N/A".to_string());

        vec![
            Line::from(vec![Span::styled(
                "Selected Trade",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("Symbol:     "),
                Span::styled(
                    trade.symbol.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![Span::raw(format!("Price:      ${:.2}", price))]),
            Line::from(vec![Span::raw(format!("Quantity:   {:.4}", qty))]),
            Line::from(vec![Span::raw(format!("Notional:   ${:.2}", notional))]),
            Line::from(vec![Span::raw(format!("Time:       {}", time))]),
        ]
    } else {
        vec![Line::from(vec![Span::styled(
            "No trade selected",
            Style::default().fg(Color::Gray),
        )])]
    };

    let block = Block::default().borders(Borders::ALL).title("Trade Detail");

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    let total_volume: f64 = app.trade_stats.values().map(|s| s.volume).sum();

    let avg_price: f64 = if app.recent_trades.is_empty() {
        0.0
    } else {
        app.recent_trades
            .iter()
            .filter_map(|t| t.price.to_f64())
            .sum::<f64>()
            / app.recent_trades.len() as f64
    };

    let stats_text = format!(
        "📊 Total: {} trades | 📈 Avg: ${:.2} | 💰 Volume: {:.2}",
        app.trade_count, avg_price, total_volume
    );

    let paragraph = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL).title("Statistics"));

    f.render_widget(paragraph, area);
}
