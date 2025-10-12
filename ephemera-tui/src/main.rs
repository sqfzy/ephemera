use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use eyre::Result;
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::time::{Duration, interval};

mod app;
mod events;
mod ui;

use app::App;
use events::EventHandler;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new().await?;
    let mut event_handler = EventHandler::new();
    
    // Tick interval for UI updates and stats
    let mut tick_interval = interval(Duration::from_secs(1));

    // Run app
    let res = run_app(&mut terminal, &mut app, &mut event_handler, &mut tick_interval).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_handler: &mut EventHandler,
    tick_interval: &mut tokio::time::Interval,
) -> Result<()> {
    loop {
        // 只在需要时渲染
        if app.can_render() {
            terminal.draw(|f| ui::layout::render(f, app))?;
        }

        tokio::select! {
            // 键盘事件具有最高优先级
            biased;
            
            // Handle keyboard events (highest priority)
            Some(event) = event_handler.next() => {
                if app.handle_event(event)? {
                    return Ok(());
                }
            }
            
            // Tick for periodic updates
            _ = tick_interval.tick() => {
                app.on_tick();
            }
            
            // Handle trade data stream
            Some(trade_result) = async {
                if let Some(stream) = &mut app.trade_stream {
                    stream.next().await
                } else {
                    None
                }
            } => {
                app.handle_trade_data(trade_result)?;
            }
            
            // Handle candle data stream
            Some(candle_result) = async {
                if let Some(stream) = &mut app.candle_stream {
                    stream.next().await
                } else {
                    None
                }
            } => {
                app.handle_candle_data(candle_result)?;
            }

            // Handle order book data stream
            Some(book_result) = async {
                if let Some(stream) = &mut app.book_stream {
                    stream.next().await
                } else {
                    None
                }
            } => {
                app.handle_book_data(book_result)?;
            }
        }
    }
}
