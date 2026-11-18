//! The image look's colours: the terminal's own (its foreground, background and the
//! palette's red, green, yellow and blue, asked by OSC 10, 11 and 4), so the image
//! follows the Rex theme; the colours the terminal doesn't say are xterm's. What each
//! colour means is the text board's (ui/mod.rs): amber needs you, red stopped or high
//! risk, green ready or low risk, blue in review. The image's background is clear, so
//! the pane's own shows through; dim text and the selected card are the foreground,
//! faint, and a task that needs you is washed with its colour.

use crate::ui::rows::Flag;

/// A colour as the terminal says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The colours the image is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Theme {
    pub fg: Rgb,
    pub bg: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
}

impl Default for Theme {
    /// A dark terminal's, for what the terminal doesn't say.
    fn default() -> Theme {
        Theme {
            fg: Rgb(0xe5, 0xe5, 0xe5),
            bg: Rgb(0x1e, 0x1e, 0x1e),
            red: Rgb(0xf1, 0x4c, 0x4c),
            green: Rgb(0x23, 0xd1, 0x8b),
            yellow: Rgb(0xf5, 0xc5, 0x42),
            blue: Rgb(0x3b, 0x8e, 0xea),
        }
    }
}

impl Theme {
    /// A light terminal's (GitHub's light colours): `--render --theme light`, for
    /// checking the image on white.
    pub fn light() -> Theme {
        Theme {
            fg: Rgb(0x24, 0x29, 0x2f),
            bg: Rgb(0xff, 0xff, 0xff),
            red: Rgb(0xcf, 0x22, 0x2e),
            green: Rgb(0x1a, 0x7f, 0x37),
            yellow: Rgb(0x9a, 0x67, 0x00),
            blue: Rgb(0x09, 0x69, 0xda),
        }
    }

    /// The theme `--theme` names: light, or dark (the default's).
    pub fn named(name: &str) -> Option<Theme> {
        match name {
            "light" => Some(Theme::light()),
            "dark" => Some(Theme::default()),
            _ => None,
        }
    }
}

/// What the terminal said about its colours (None: it didn't).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Said {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    /// palette entries by index (1 red, 2 green, 3 yellow, 4 blue)
    pub palette: Vec<(u8, Rgb)>,
}

impl Theme {
    /// This theme with what the terminal said over it.
    pub fn with(self, said: &Said) -> Theme {
        let pal = |i: u8, d: Rgb| said.palette.iter().find(|(j, _)| *j == i).map_or(d, |(_, c)| *c);
        Theme {
            fg: said.fg.unwrap_or(self.fg),
            bg: said.bg.unwrap_or(self.bg),
            red: pal(1, self.red),
            green: pal(2, self.green),
            yellow: pal(3, self.yellow),
            blue: pal(4, self.blue),
        }
    }

    /// The foreground, at full strength.
    pub fn text(&self) -> [u8; 4] {
        solid(self.fg)
    }

    /// Dim text: the foreground at 66%, which reads at 4.5:1 on a light theme's white,
    /// even on the selected card (and at over 6:1 on a dark one).
    pub fn dim(&self) -> [u8; 4] {
        faint(self.fg, 0.66)
    }

    /// A group's name: the foreground, a little dim, so the titles under it lead.
    pub fn heading(&self) -> [u8; 4] {
        faint(self.fg, 0.82)
    }

    /// The selected card's fill: the foreground, faintly.
    pub fn tint(&self) -> [u8; 4] {
        faint(self.fg, 0.085)
    }

    /// The selected card's edge: the foreground, a little stronger than its fill.
    pub fn edge(&self) -> [u8; 4] {
        faint(self.fg, 0.22)
    }

    /// A flag's colour: amber for a task that needs you, red for one whose agent stopped.
    pub fn flag(&self, flag: Flag) -> Rgb {
        match flag {
            Flag::Needs => self.yellow,
            Flag::Stopped => self.red,
        }
    }

    /// Behind a task that needs you or stopped: its flag's colour, faintly, so dim text
    /// on it still reads at 4.5:1 on a light theme's white.
    pub fn wash(&self, flag: Flag) -> [u8; 4] {
        faint(self.flag(flag), 0.075)
    }

    /// A status's colour, as ui::mark_style gives it; None for the dim ones.
    pub fn status(&self, status: &str) -> Option<[u8; 4]> {
        match status {
            "needs" => Some(solid(self.yellow)),
            "stopped" => Some(solid(self.red)),
            "ready" | "finished" => Some(solid(self.green)),
            "review" => Some(solid(self.blue)),
            _ => None,
        }
    }

    /// A risk level's colour, as ui::risk_style gives it.
    pub fn risk(&self, level: &str) -> [u8; 4] {
        match level {
            "high" => solid(self.red),
            "medium" => solid(self.yellow),
            _ => solid(self.green),
        }
    }
}

pub fn solid(c: Rgb) -> [u8; 4] {
    [c.0, c.1, c.2, 255]
}

pub fn faint(c: Rgb, alpha: f32) -> [u8; 4] {
    [c.0, c.1, c.2, (alpha * 255.0).round() as u8]
}

/// A colour as an OSC answer gives it: "rgb:RRRR/GGGG/BBBB" (1 to 4 hex digits each),
/// or "#RRGGBB".
pub fn parse_color(s: &str) -> Option<Rgb> {
    if let Some(hex) = s.strip_prefix('#') {
        let v = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return (hex.len() == 6).then(|| Some(Rgb(v(0)?, v(2)?, v(4)?))).flatten();
    }
    let rest = s.strip_prefix("rgba:").or_else(|| s.strip_prefix("rgb:"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    let one = |p: &str| -> Option<u8> {
        if p.is_empty() || p.len() > 4 {
            return None;
        }
        let v = u32::from_str_radix(p, 16).ok()?;
        let max = (1u32 << (4 * p.len())) - 1;
        Some((v * 255 / max) as u8)
    };
    Some(Rgb(one(parts[0])?, one(parts[1])?, one(parts[2])?))
}

#[cfg(test)]
#[path = "tests/test_theme.rs"]
mod tests;
