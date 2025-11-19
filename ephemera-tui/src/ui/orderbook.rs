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
    let symbol = app.get_selected_symbol();

    if let Some(book) = app.order_books.get(symbol) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(area);

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[0]);

        render_asks(f, book, main_chunks[0]);
        render_bids(f, book, main_chunks[1]);
        render_spread_info(f, book, chunks[1]);
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("Order Book - {} (Press 1/2/3 to switch)", symbol));

        let paragraph = Paragraph::new("⏳ Waiting for order book data...")
            .block(block)
            .style(Style::default().fg(Color::Gray));

        f.render_widget(paragraph, area);
    }
}

fn render_asks(f: &mut Frame, book: &ephemera_shared::BookData, area: Rect) {
    let header = Row::new(vec!["Price", "Amount", "Total"])
        .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let max_amount = book
        .asks
        .iter()
        .map(|(_, amt)| amt.to_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max);

    let rows: Vec<Row> = book
        .asks
        .iter()
        .take(10)
        .map(|(price, amount)| {
            let price_f = price.to_f64().unwrap_or(0.0);
            let amount_f = amount.to_f64().unwrap_or(0.0);
            let total = price_f * amount_f;

            // Calculate bar width (0-100%)
            let bar_pct = if max_amount > 0.0 {
                (amount_f / max_amount * 100.0) as usize
            } else {
                0
            };
            let bar = "█".repeat(bar_pct.min(20) / 5);

            Row::new(vec![
                format!("{:.2}", price_f),
                format!("{:.4} {}", amount_f, bar),
                format!("{:.2}", total),
            ])
            .style(Style::default().fg(Color::Red))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title("Asks (Sell) 🔴"),
    );

    f.render_widget(table, area);
}

fn render_bids(f: &mut Frame, book: &ephemera_shared::BookData, area: Rect) {
    let header = Row::new(vec!["Price", "Amount", "Total"])
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let max_amount = book
        .bids
        .iter()
        .map(|(_, amt)| amt.to_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max);

    let rows: Vec<Row> = book
        .bids
        .iter()
        .take(10)
        .map(|(price, amount)| {
            let price_f = price.to_f64().unwrap_or(0.0);
            let amount_f = amount.to_f64().unwrap_or(0.0);
            let total = price_f * amount_f;

            // Calculate bar width (0-100%)
            let bar_pct = if max_amount > 0.0 {
                (amount_f / max_amount * 100.0) as usize
            } else {
                0
            };
            let bar = "█".repeat(bar_pct.min(20) / 5);

            Row::new(vec![
                format!("{:.2}", price_f),
                format!("{:.4} {}", amount_f, bar),
                format!("{:.2}", total),
            ])
            .style(Style::default().fg(Color::Green))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title("Bids (Buy) 🟢"),
    );

    f.render_widget(table, area);
}

fn render_spread_info(f: &mut Frame, book: &ephemera_shared::BookData, area: Rect) {
    let best_ask = book
        .asks
        .first()
        .and_then(|(p, _)| p.to_f64())
        .unwrap_or(0.0);

    let best_bid = book
        .bids
        .first()
        .and_then(|(p, _)| p.to_f64())
        .unwrap_or(0.0);

    let mid_price = (best_ask + best_bid) / 2.0;
    let spread = best_ask - best_bid;
    let spread_pct = if mid_price > 0.0 {
        (spread / mid_price) * 100.0
    } else {
        0.0
    };

    let info = Line::from(vec![
        Span::raw(format!("Mid Price: ${:.2} ", mid_price)),
        Span::raw("| "),
        Span::raw(format!("Spread: ${:.2} ", spread)),
        Span::raw(format!("({:.3}%)", spread_pct)),
    ]);

    let paragraph =
        Paragraph::new(info).block(Block::default().borders(Borders::ALL).title("Market Info"));

    f.render_widget(paragraph, area);
}
