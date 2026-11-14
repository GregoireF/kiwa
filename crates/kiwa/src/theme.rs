//! Colour and text attributes for the whole interface, decided here and nowhere else:
//! - the accent marks the cursor position: selected row, active scope, prompt; a dimmed
//!   variant draws the borders;
//! - green, yellow and red carry results (passed, warning, failed) and nothing else;
//! - hierarchy comes from bold and the terminal's own faint attribute (SGR 2), which adapts
//!   to light and dark themes, rather than a fixed grey;
//! - `NO_COLOR` (https://no-color.org) removes colours but keeps bold, faint and underline,
//!   and nothing is ever conveyed by colour alone (symbols ✔ ✘ ⚠ and text labels).

use ratatui::style::{Color, Modifier, Style};

/// Maps an accent name to a terminal colour. Apart from the ANSI cyan default, each accent is
/// an xterm-256 index, chosen to stay readable on both light and dark backgrounds.
/// `none` has no colour; unknown names fall back to the default.
pub fn accent_color(name: &str) -> Option<Color> {
    Some(match name {
        "azure" => Color::Indexed(39),
        "indigo" => Color::Indexed(105),
        "violet" => Color::Indexed(171),
        "rose" => Color::Indexed(204),
        "amber" => Color::Indexed(214),
        "steel" => Color::Indexed(110),
        "none" => return None,
        _ => Color::Cyan,
    })
}

pub fn colors_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub color: bool,
    pub accent: Style,
    pub border: Style,
    pub dim: Style,
    pub bold: Style,
    pub ok: Style,
    pub err: Style,
    pub warn: Style,
    pub branch_main: Style,
    pub branch_other: Style,
    pub hover: Style,
    pub matched: Style,
}

impl Theme {
    pub fn new(accent: Option<&str>, color: bool) -> Self {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let dim = Style::new().add_modifier(Modifier::DIM);
        let fg = |c: Color| {
            if color {
                Style::new().fg(c)
            } else {
                Style::new()
            }
        };
        let accent_fg = if color {
            accent_color(accent.unwrap_or("cyan"))
        } else {
            None
        };
        let accent_style = match accent_fg {
            Some(c) => Style::new().fg(c).add_modifier(Modifier::BOLD),
            None => bold,
        };
        Self {
            color,
            accent: accent_style,
            border: match accent_fg {
                Some(c) => Style::new().fg(c).add_modifier(Modifier::DIM),
                None => dim,
            },
            dim,
            bold,
            ok: fg(Color::Green),
            err: fg(Color::Red),
            warn: fg(Color::Yellow),
            // Branches: low-saturation xterm-256 tones, so they never read as a pass or a fail.
            branch_main: fg(Color::Indexed(108)),
            branch_other: fg(Color::Indexed(179)),
            hover: if color {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::new()
            },
            matched: Style::new().add_modifier(Modifier::UNDERLINED),
        }
    }

    pub fn from_env(accent: Option<&str>) -> Self {
        Self::new(accent, colors_enabled())
    }
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn time_ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// `D:\very\long\path\to\project` → `D:\very\…\to\project`, keeping both ends.
pub fn truncate_middle(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        return s.to_owned();
    }
    if width <= 1 {
        return "…".into();
    }
    let keep = width - 1;
    let head = keep / 2;
    let tail = keep - head;
    let chars: Vec<char> = s.chars().collect();
    format!(
        "{}…{}",
        chars[..head].iter().collect::<String>(),
        chars[n - tail..].iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_config_accent_has_a_colour_except_none() {
        for a in kiwa_core::config::ACCENTS {
            assert_eq!(accent_color(a).is_none(), *a == "none", "{a}");
        }
        assert_eq!(accent_color("unknown"), Some(Color::Cyan));
    }

    #[test]
    fn no_color_keeps_attributes_only() {
        let t = Theme::new(Some("amber"), false);
        assert_eq!(t.accent.fg, None);
        assert!(t.accent.add_modifier.contains(Modifier::BOLD));
        assert_eq!(t.ok.fg, None);
        assert_eq!(t.hover.bg, None);
        let c = Theme::new(Some("amber"), true);
        assert_eq!(c.accent.fg, Some(Color::Indexed(214)));
        assert!(c.border.add_modifier.contains(Modifier::DIM));
        let b = Theme::new(Some("none"), true);
        assert_eq!(b.accent.fg, None);
    }

    #[test]
    fn durations_and_paths() {
        assert_eq!(time_ago(5), "just now");
        assert_eq!(time_ago(125), "2 min ago");
        assert_eq!(time_ago(7_300), "2 h ago");
        assert_eq!(time_ago(200_000), "2 d ago");
        assert_eq!(truncate_middle("abcdefghij", 7), "abc…hij");
        assert_eq!(truncate_middle("short", 10), "short");
        assert_eq!(truncate_middle("abc", 1), "…");
    }
}
