use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
};
use rust_decimal::prelude::ToPrimitive;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    render_chart(f, app, chunks[0]);
    render_ohlc_info(f, app, chunks[1]);
}

fn render_chart(f: &mut Frame, app: &App, area: Rect) {
    let symbol = app.get_selected_symbol();

    if app.candle_data.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("Candles - {} [1m] (Press 1/2/3 to switch)", symbol));
        let paragraph = Paragraph::new("⏳ No candle data available yet...")
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    let candles = app.candle_data.get(symbol);

    if candles.is_none() || candles.unwrap().is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("Candles - {} [1m] (Press 1/2/3 to switch)", symbol));
        let paragraph = Paragraph::new(format!("⏳ Waiting for {} candle data...", symbol))
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    let candles = candles.unwrap();

    let data: Vec<(f64, f64)> = candles
        .iter()
        .enumerate()
        .filter_map(|(i, candle)| candle.close.to_f64().map(|price| (i as f64, price)))
        .collect();

    if data.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("Candles - {} [1m] (Press 1/2/3 to switch)", symbol));
        let paragraph = Paragraph::new("⏳ Processing candle data...")
            .block(block)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    let min_price = data.iter().map(|(_, p)| *p).fold(f64::INFINITY, f64::min);
    let max_price = data
        .iter()
        .map(|(_, p)| *p)
        .fold(f64::NEG_INFINITY, f64::max);
    let price_range = max_price - min_price;
    let y_min = min_price - price_range * 0.1;
    let y_max = max_price + price_range * 0.1;

    // Determine color based on first and last price
    let first_price = data.first().map(|(_, p)| *p).unwrap_or(0.0);
    let last_price = data.last().map(|(_, p)| *p).unwrap_or(0.0);
    let line_color = if last_price > first_price {
        Color::Green
    } else if last_price < first_price {
        Color::Red
    } else {
        Color::Gray
    };

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(line_color))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Candles - {} [1m] (Press 1/2/3 to switch)", symbol)),
        )
        .x_axis(
            Axis::default()
                .title("Time (candles)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, data.len() as f64]),
        )
        .y_axis(
            Axis::default()
                .title("Price")
                .style(Style::default().fg(Color::Gray))
                .bounds([y_min, y_max])
                .labels([
                    format!("{:.2}", y_min),
                    format!("{:.2}", (y_min + y_max) / 2.0),
                    format!("{:.2}", y_max),
                ]),
        );

    f.render_widget(chart, area);
}

fn render_ohlc_info(f: &mut Frame, app: &App, area: Rect) {
    let symbol = app.get_selected_symbol();

    let info_text = if let Some(candles) = app.candle_data.get(symbol) {
        if let Some(latest) = candles.back() {
            let open = latest.open.to_f64().unwrap_or(0.0);
            let high = latest.high.to_f64().unwrap_or(0.0);
            let low = latest.low.to_f64().unwrap_or(0.0);
            let close = latest.close.to_f64().unwrap_or(0.0);
            let change_pct = if open > 0.0 {
                ((close - open) / open) * 100.0
            } else {
                0.0
            };

            let (change_icon, change_color) = if change_pct > 0.0 {
                ("📈", Color::Green)
            } else if change_pct < 0.0 {
                ("📉", Color::Red)
            } else {
                ("➖", Color::Gray)
            };

            vec![Line::from(vec![
                Span::raw(format!("O: ${:.2} ", open)),
                Span::raw(format!("H: ${:.2} ", high)),
                Span::raw(format!("L: ${:.2} ", low)),
                Span::raw(format!("C: ${:.2} ", close)),
                Span::raw("| "),
                Span::styled(
                    format!("{} {:+.2}%", change_icon, change_pct),
                    Style::default()
                        .fg(change_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ])]
        } else {
            vec![Line::from("No OHLC data available")]
        }
    } else {
        vec![Line::from("No OHLC data available")]
    };

    let paragraph =
        Paragraph::new(info_text).block(Block::default().borders(Borders::ALL).title("OHLC Info"));

    f.render_widget(paragraph, area);
}

