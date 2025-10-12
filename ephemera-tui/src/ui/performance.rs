use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    render_latency_chart(f, app, chunks[0]);
    render_throughput_chart(f, app, chunks[1]);
    render_stats_summary(f, app, chunks[2]);
}

fn render_latency_chart(f: &mut Frame, app: &App, area: Rect) {
    if app.performance_history.is_empty() {
        let paragraph = Paragraph::new("⏳ Collecting performance data...")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Network Latency"),
            )
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    let data: Vec<(f64, f64)> = app
        .performance_history
        .iter()
        .enumerate()
        .map(|(i, sample)| (i as f64, sample.latency_ms))
        .collect();

    let max_latency = data.iter().map(|(_, l)| *l).fold(0.0_f64, f64::max);
    // let min_latency = data.iter().map(|(_, l)| *l).fold(f64::INFINITY, f64::min);

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Network Latency (ms)"),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, data.len() as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_latency * 1.2])
                .labels([
                    "0".to_string(),
                    format!("{:.0}", max_latency / 2.0),
                    format!("{:.0}", max_latency),
                ]),
        );

    f.render_widget(chart, area);
}

fn render_throughput_chart(f: &mut Frame, app: &App, area: Rect) {
    if app.performance_history.is_empty() {
        let paragraph = Paragraph::new("⏳ Collecting performance data...")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Message Throughput"),
            )
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, area);
        return;
    }

    let data: Vec<(f64, f64)> = app
        .performance_history
        .iter()
        .enumerate()
        .map(|(i, sample)| (i as f64, sample.trades_per_sec))
        .collect();

    let max_throughput = data.iter().map(|(_, t)| *t).fold(0.0_f64, f64::max);

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Green))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Message Rate (trades/sec)"),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, data.len() as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_throughput * 1.2])
                .labels([
                    "0".to_string(),
                    format!("{:.0}", max_throughput / 2.0),
                    format!("{:.0}", max_throughput),
                ]),
        );

    f.render_widget(chart, area);
}

fn render_stats_summary(f: &mut Frame, app: &App, area: Rect) {
    let avg_latency = if app.performance_history.is_empty() {
        0.0
    } else {
        app.performance_history
            .iter()
            .map(|s| s.latency_ms)
            .sum::<f64>()
            / app.performance_history.len() as f64
    };

    let p99_latency = if app.performance_history.is_empty() {
        0.0
    } else {
        let mut latencies: Vec<f64> = app
            .performance_history
            .iter()
            .map(|s| s.latency_ms)
            .collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p99_idx = ((latencies.len() as f64) * 0.99) as usize;
        latencies.get(p99_idx).copied().unwrap_or(0.0)
    };

    let data_rate_mbps = (app.system_stats.trades_per_sec * 500.0) / 1_000_000.0; // Assume ~500 bytes per trade

    let lines = vec![
        Line::from(vec![Span::styled(
            "Performance Summary",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(format!(
            "📊 Avg Latency:      {:.2} ms",
            avg_latency
        ))]),
        Line::from(vec![Span::raw(format!(
            "📊 P99 Latency:      {:.2} ms",
            p99_latency
        ))]),
        Line::from(vec![Span::raw(format!(
            "📈 Total Messages:   {}",
            app.trade_count
        ))]),
        Line::from(vec![Span::raw(format!(
            "🚀 Data Rate:        {:.2} MB/s",
            data_rate_mbps
        ))]),
        Line::from(vec![Span::raw(format!(
            "⏱️  Uptime:           {}s",
            chrono::Utc::now()
                .signed_duration_since(app.start_time)
                .num_seconds()
        ))]),
    ];

    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Statistics"));

    f.render_widget(paragraph, area);
}
