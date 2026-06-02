pub mod dashboard;
pub mod details;
pub mod issue_list;
pub mod markdown;
pub mod modal;
pub mod repo_panel;

use csscolorparser::parse;
use ratatui::layout::Rect;
use ratatui::style::Color;

/// Return the inner [`Rect`] of a single-border block — subtracts one cell on
/// every side. Use this instead of manually computing `x+1, y+1, w-2, h-2`.
pub fn inner_rect(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Parse a CSS colour string (with or without a leading `#`) into a ratatui `Color::Rgb`.
/// Returns `None` if the string is empty or cannot be parsed.
pub fn parse_css_color_to_rgb(colour_str: &str) -> Option<Color> {
    if colour_str.trim().is_empty() {
        return None;
    }
    if let Ok(col) = parse(colour_str.trim()) {
        let [red, green, blue, _alpha] = col.to_rgba8();
        return Some(Color::Rgb(red, green, blue));
    }
    if let Ok(col) = parse(&format!("#{}", colour_str.trim())) {
        let [red, green, blue, _alpha] = col.to_rgba8();
        return Some(Color::Rgb(red, green, blue));
    }
    None
}
