use crossterm::event::{self, Event, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct EventHandler {
    receiver: mpsc::UnboundedReceiver<KeyEvent>,
    _handle: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let handle = tokio::spawn(async move {
            loop {
                // 阻塞等待事件，避免 busy-waiting
                if let Ok(true) = event::poll(Duration::from_millis(100))
                    && let Ok(Event::Key(key)) = event::read()
                    && sender.send(key).is_err()
                {
                    break;
                }
            }
        });

        Self {
            receiver,
            _handle: handle,
        }
    }

    pub async fn next(&mut self) -> Option<KeyEvent> {
        self.receiver.recv().await
    }
}
