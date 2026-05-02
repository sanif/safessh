use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Cyan;

pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}
