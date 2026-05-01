use crate::theme;
use ratatui::{prelude::*, widgets::Paragraph};

pub fn header(frame: &mut Frame<'_>, area: Rect, title: &str) {
    frame.render_widget(Paragraph::new(title).style(theme::title()), area);
}

pub fn footer(frame: &mut Frame<'_>, area: Rect, hints: &str) {
    frame.render_widget(Paragraph::new(hints).style(theme::dim()), area);
}
