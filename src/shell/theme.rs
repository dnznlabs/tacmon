use crate::config::ThemeConfig;
use ratatui::style::{Color, Style};
use std::str::FromStr;

pub struct Theme {
    pub accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub dim: Color,
}

impl Theme {
    /// A colour that does not parse falls back to the default for its role.
    pub fn from_config(c: &ThemeConfig) -> Self {
        let color = |s: &str, d: Color| Color::from_str(s).unwrap_or(d);
        Self {
            accent: color(&c.accent, Color::Cyan),
            good: color(&c.good, Color::Green),
            warn: color(&c.warn, Color::Yellow),
            bad: color(&c.bad, Color::Red),
            dim: color(&c.dim, Color::DarkGray),
        }
    }
    pub fn dim(&self) -> Style {
        Style::new().fg(self.dim)
    }
}
