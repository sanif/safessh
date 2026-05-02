//! Help overlay text — also exported as a public function so docs/tui.md
//! can include it verbatim and stay in sync (Task 16).

use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Canonical help text shown on `?`. Re-exported as a `pub fn` so
/// docs/tui.md can include it verbatim and stay in sync with the binary.
pub fn help_text() -> &'static str {
    "\
Global
  q / Esc      quit
  Ctrl-C       quit
  Tab          next screen
  Shift-Tab    previous screen
  ?            toggle this help

Projects
  Up / k       move selection up
  Down / j     move selection down
  i            import targets from ssh-config (Task 13)

Approvals
  Up/Down/k/j  move selection
  Enter        open action picker (Once / Timed / Always / Deny / Block)
  Esc          close picker

Rules
  < / Left     previous project
  > / Right    next project
  1 / 2 / 3    Timed / Always / Blocked tab
  Up/Down/k/j  move selection
  d            delete selected rule

Audit
  Up/Down/k/j  move selection
  g / G        jump to top / bottom (G resumes auto-scroll)
  /p           filter by project
  /t           filter by event type
  /            grep substring filter
  Esc          cancel current filter edit
"
}

pub fn render_overlay(frame: &mut Frame<'_>, area: Rect) {
    let rect = centered_rect(80, 20, area);
    frame.render_widget(Clear, rect);
    let p = Paragraph::new(help_text())
        .block(
            Block::default()
                .title("Help (? to close)")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(p, rect);
}

fn centered_rect(percent_w: u16, height: u16, area: Rect) -> Rect {
    let pop_w = (area.width * percent_w / 100).min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(pop_w)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: pop_w,
        height,
    }
}
