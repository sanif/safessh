//! Multiplexes terminal input + filesystem events + a periodic tick into a
//! single `AppEvent` stream consumed by [`crate::App::run`].

use crossterm::event::{Event as CtEvent, KeyEvent};
use safessh_core::error::Result;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEvent {
    ApprovalsChanged,
    ProjectsChanged,
    AuditAppended,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Fs(FsEvent),
}

pub struct EventStream {
    rx: mpsc::Receiver<AppEvent>,
    /// Sender passed to the filesystem watcher (Task 6) so it can inject
    /// `FsEvent`s into the same stream that drives terminal input.
    pub fs_tx: mpsc::Sender<FsEvent>,
}

impl EventStream {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(64);
        let (fs_tx, mut fs_rx) = mpsc::channel::<FsEvent>(64);

        let tick_tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                if tick_tx.send(AppEvent::Tick).await.is_err() {
                    break;
                }
            }
        });

        let key_tx = tx.clone();
        // Tokio's blocking pool blocks runtime shutdown until the task
        // returns, so we poll a short interval and check whether the
        // receiver has dropped on every iteration. Without this check the
        // thread sits in `crossterm::event::poll(100ms)` forever after the
        // event loop has broken on Quit, leaving the binary hung until the
        // user sends another signal (Ctrl-C).
        tokio::task::spawn_blocking(move || loop {
            if key_tx.is_closed() {
                break;
            }
            if crossterm::event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(CtEvent::Key(k)) = crossterm::event::read() {
                    if key_tx.blocking_send(AppEvent::Key(k)).is_err() {
                        break;
                    }
                }
            }
        });

        let fs_relay = tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = fs_rx.recv().await {
                if fs_relay.send(AppEvent::Fs(ev)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self { rx, fs_tx })
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}
