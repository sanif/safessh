//! safessh's TUI: ratatui screens that share storage with the CLI.
//!
//! All file mutations go through the same `safessh-storage` API the CLI
//! uses (PendingStore, AlwaysStore, etc.) so atomic-write + locking
//! semantics are preserved (SAFETY-INVARIANT-12).

mod app;
pub mod event;
pub mod help;
pub mod screens;
mod theme;
pub mod watcher;
mod widgets;

pub use app::{run, App, AppAction};
pub use event::{AppEvent, EventStream, FsEvent};
pub use help::help_text;
