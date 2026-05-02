use crate::theme;
use chrono::{DateTime, Utc};
use ratatui::{prelude::*, widgets::Paragraph};

pub fn header(frame: &mut Frame<'_>, area: Rect, title: &str) {
    frame.render_widget(Paragraph::new(title).style(theme::title()), area);
}

pub fn footer(frame: &mut Frame<'_>, area: Rect, hints: &str) {
    frame.render_widget(Paragraph::new(hints).style(theme::dim()), area);
}

/// Transient banner shown along the top edge of the screen. The App
/// owns a `Toast` and clears it on tick once `expires_at < now`.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub expires_at: DateTime<Utc>,
}

impl Toast {
    pub fn new(text: impl Into<String>, lifetime: chrono::Duration) -> Self {
        Self {
            text: text.into(),
            expires_at: Utc::now() + lifetime,
        }
    }

    /// Returns true if the toast has expired and should be cleared.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

pub fn render_toast(frame: &mut Frame<'_>, area: Rect, toast: &Toast) {
    let bar = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let p = Paragraph::new(toast.text.clone()).style(
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::REVERSED),
    );
    frame.render_widget(p, bar);
}
