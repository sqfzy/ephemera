use ratatui::{
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let log_lines: Vec<ratatui::text::Line> = app.logs
        .iter()
        .map(|log| {
            let style = if log.contains("🔴") || log.contains("❌") {
                Style::default().fg(Color::Red)
            } else if log.contains("🟢") {
                Style::default().fg(Color::Green)
            } else if log.contains("🟡") || log.contains("⚠️") {
                Style::default().fg(Color::Yellow)
            } else if log.contains("📈") || log.contains("📊") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            
            ratatui::text::Line::from(log.as_str()).style(style)
        })
        .collect();

    let total_lines = log_lines.len() as u16;
    let visible_lines = area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let scroll = app.logs_scroll.min(max_scroll);

    let paragraph = Paragraph::new(log_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("System Logs (Key events, errors, and statistics) - Scroll: {}/{}", scroll, max_scroll)))
        .scroll((scroll, 0));

    f.render_widget(paragraph, area);

    // 渲染滚动条
    if total_lines > visible_lines {
        let mut scrollbar_state = ScrollbarState::default()
            .content_length(total_lines as usize)
            .position(scroll as usize);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        f.render_stateful_widget(
            scrollbar,
            area,
            &mut scrollbar_state,
        );
    }
}
