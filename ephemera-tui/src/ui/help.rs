use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let help_text = vec![
        Line::from(vec![
            Span::styled("⚡ EPHEMERA TUI - HELP", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 NAVIGATION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  Tab          - Next tab"),
        Line::from("  Shift+Tab    - Previous tab"),
        Line::from("  ↑/↓ or j/k   - Navigate lists / Scroll help"),
        Line::from("  PageUp/Down  - Fast scroll"),
        Line::from("  Home/End     - Jump to top/bottom"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 QUICK JUMP", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  d - Dashboard    t - Trades       c - Candles"),
        Line::from("  o - OrderBook    p - Performance  l - Logs"),
        Line::from("  h - Help"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 SYMBOLS", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  1 - BTC-USDT    2 - ETH-USDT    3 - SOL-USDT"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 CONTROLS", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  Space - ⏸ Pause/Resume streams"),
        Line::from("  a     - 🔔 Toggle alerts panel"),
        Line::from("  q     - 🚪 Quit"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 TABS OVERVIEW", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  📊 Dashboard", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - Real-time price cards for all symbols"),
        Line::from("     - Sparkline trend visualization"),
        Line::from("     - Price change indicators"),
        Line::from("     - System connection status"),
        Line::from("     - Recent significant trades (Volume > 1.0)"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  💹 Trades", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - Real-time trade stream display"),
        Line::from("     - Dual-pane layout (list + detail)"),
        Line::from("     - Select trades with ↑/↓ or j/k"),
        Line::from("     - View detailed trade information"),
        Line::from("     - Bottom statistics bar"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  📈 Candles", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - 1-minute candlestick chart"),
        Line::from("     - Switch symbols with 1/2/3 keys"),
        Line::from("     - OHLC (Open/High/Low/Close) data"),
        Line::from("     - Price change percentage"),
        Line::from("     - Dynamic color (green=up, red=down)"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  📖 OrderBook", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - Real-time market depth"),
        Line::from("     - Top 10 bids (buy orders) in green"),
        Line::from("     - Top 10 asks (sell orders) in red"),
        Line::from("     - Visual depth bars"),
        Line::from("     - Mid price and spread calculation"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ⚡ Performance", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - Network latency monitoring"),
        Line::from("     - Message throughput chart"),
        Line::from("     - Average and P99 latency"),
        Line::from("     - Data rate statistics"),
        Line::from("     - System uptime"),
        Line::from(""),
        Line::from(vec![
            Span::styled("  📝 Logs", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("     - System events and errors"),
        Line::from("     - Connection status changes"),
        Line::from("     - Statistical summaries"),
        Line::from("     - Color-coded by severity"),
        Line::from("     - Scrollable with ↑/↓"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 FEATURES", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  🔔 Smart Alerts"),
        Line::from("     - Large trade detection (volume > 5.0)"),
        Line::from("     - Price spike alerts (change > 2%)"),
        Line::from("     - Three severity levels: Info/Warning/Critical"),
        Line::from("     - Toggle visibility with 'a' key"),
        Line::from(""),
        Line::from("  📊 Real-time Statistics"),
        Line::from("     - Trades per second"),
        Line::from("     - Total volume tracking"),
        Line::from("     - Price change percentages"),
        Line::from("     - Peak throughput monitoring"),
        Line::from(""),
        Line::from("  ⏸ Pause/Resume"),
        Line::from("     - Freeze data streams for inspection"),
        Line::from("     - Resume without reconnection"),
        Line::from("     - Useful for detailed analysis"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 DATA SOURCE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  Exchange:  OKX (via WebSocket)"),
        Line::from("  Symbols:   BTC-USDT, ETH-USDT, SOL-USDT"),
        Line::from("  Streams:   Trade, Candle (1min), OrderBook (5 levels)"),
        Line::from(""),
        Line::from(vec![
            Span::styled("🔹 TIPS", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("  • Use Space to pause when you need to inspect data"),
        Line::from("  • Press 'a' to hide alerts if they're distracting"),
        Line::from("  • PageUp/PageDown for fast scrolling in Help/Logs"),
        Line::from("  • Home/End to jump to top/bottom quickly"),
        Line::from("  • Watch the Performance tab to monitor system health"),
        Line::from(""),
        Line::from(vec![
            Span::styled("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("Ephemera TUI v0.1.0", Style::default().fg(Color::Gray)),
            Span::raw(" | "),
            Span::styled("Built with Rust + Ratatui", Style::default().fg(Color::Gray)),
        ]),
    ];

    let total_lines = help_text.len() as u16;
    let visible_lines = area.height.saturating_sub(2); // 减去边框
    
    // 限制滚动位置
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let scroll = app.help_scroll.min(max_scroll);

    let paragraph = Paragraph::new(help_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("Help & Keyboard Shortcuts (Scroll: {}/{})", scroll, max_scroll)))
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
