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

    /// Dim text: the foreground at 70%, which reads at over 4.5:1 on a light theme's
    /// white, even on the selected card (and at over 6:1 on a dark one).
    pub fn dim(&self) -> [u8; 4] {
        faint(self.fg, 0.7)
    }

    /// A group's name: the foreground, a little dim, so the titles under it lead.
    pub fn heading(&self) -> [u8; 4] {
        faint(self.fg, 0.82)
    }

    /// The title of a task that isn't waiting on you (working, in review, queued): the
    /// foreground a step back, so the titles that are yours lead the list.
    pub fn soft(&self) -> [u8; 4] {
        faint(self.fg, 0.86)
    }

    /// A quiet title (the coordinator's task, a closed one): dimmer than the rest, a
    /// step above its own lines.
    pub fn quiet(&self) -> [u8; 4] {
        faint(self.fg, 0.76)
    }

    /// The selected card's fill: the foreground, faintly.
    pub fn tint(&self) -> [u8; 4] {
        faint(self.fg, 0.085)
    }

    /// The selected card's edge: the terminal's blue, whatever the card's fill. Nothing
    /// else on the board is boxed, so the ring is the selection wherever it is, among
    /// amber and red washes too.
    pub fn edge(&self) -> [u8; 4] {
        solid(self.blue)
    }

    /// The selected card's fill when its task needs you or stopped: its flag's colour,
    /// well over twice as strong as its wash.
    pub fn flag_tint(&self, flag: Flag) -> [u8; 4] {
        faint(self.fill(flag), 0.26)
    }



    /// The rule under the top line and over the bottom rows: the foreground, faintly.
    pub fn rule(&self) -> [u8; 4] {
        faint(self.fg, 0.12)
    }

    /// A key's cap, and its edge: the foreground, faintly.
    pub fn cap(&self) -> [u8; 4] {
        faint(self.fg, 0.07)
    }

    pub fn cap_edge(&self) -> [u8; 4] {
        faint(self.fg, 0.28)
    }

    /// A flag's colour: amber for a task that needs you, red for one whose agent stopped.
    pub fn flag(&self, flag: Flag) -> Rgb {
        match flag {
            Flag::Needs => self.yellow,
            Flag::Stopped => self.red,
        }
    }

    /// A flag's colour as a fill: amber a quarter of the way to red (a faint yellow on
    /// a dark pane is olive; this stays warm), at its brightest.
    fn fill(&self, flag: Flag) -> Rgb {
        let warm = |y: u8, r: u8| ((y as u16 * 3 + r as u16) / 4) as u8;
        bright(match flag {
            Flag::Needs => Rgb(warm(self.yellow.0, self.red.0), warm(self.yellow.1, self.red.1), warm(self.yellow.2, self.red.2)),
            Flag::Stopped => self.red,
        })
    }

    /// Behind a task that needs you or stopped: its flag's colour at its brightest (a
    /// light theme's amber is dark, to read as text, and a wash of it is beige),
    /// faintly, so dim text on it still reads at 4.5:1 on a light theme's white.
    pub fn wash(&self, flag: Flag) -> [u8; 4] {
        faint(self.fill(flag), 0.1)
    }

    /// A status's colour for a shape (a mark, a segment of the status bar, a dot): its
    /// colour, lifted (`lit`); None for the dim ones.
    pub fn shape(&self, status: &str) -> Option<[u8; 4]> {
        self.status(status).map(|c| solid(lit(Rgb(c[0], c[1], c[2]))))
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

    /// A group's colour in the header's status bar, by its first status: what waits on
    /// you in its status's colour (finished the ready's green, drawn hollow), in review blue, the
    /// coordinator's a violet of its own, and what's working or queued in the
    /// foreground, the queued fainter.
    pub fn group(&self, status: &str) -> [u8; 4] {
        match status {
            "needs" | "stopped" | "ready" | "finished" | "review" => self.shape(status).unwrap_or(self.text()),
            "working" => faint(self.fg, 0.7),
            "triage" => solid(self.between()),
            _ => faint(self.fg, 0.45),
        }
    }

    /// A violet between the terminal's red and blue, toward the foreground: the
    /// coordinator's tasks in the status bar, which are neither yours nor a worker's.
    fn between(&self) -> Rgb {
        let mix = |a: u8, b: u8, c: u8| ((a as u16 * 3 + b as u16 * 4 + c as u16) / 8) as u8;
        Rgb(mix(self.red.0, self.blue.0, self.fg.0), mix(self.red.1, self.blue.1, self.fg.1), mix(self.red.2, self.blue.2, self.fg.2))
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

/// A colour for a shape: a light theme's colours are dark, to read as text on white,
/// and a mark or a bar in them is dull (its amber is brown), so a colour whose
/// strongest part is under 200 is scaled up to that. A dark theme's are left as they are.
pub fn lit(c: Rgb) -> Rgb {
    let most = c.0.max(c.1).max(c.2).max(1) as u32;
    if most >= 200 {
        return c;
    }
    let up = |v: u8| (v as u32 * 200 / most) as u8;
    Rgb(up(c.0), up(c.1), up(c.2))
}

/// A colour at its brightest: its hue, scaled so its strongest part is whole. What a
/// fill is made of (a wash, a chip): a faint fill of a dark colour is muddy.
pub fn bright(c: Rgb) -> Rgb {
    let most = c.0.max(c.1).max(c.2).max(1) as u32;
    let up = |v: u8| (v as u32 * 255 / most) as u8;
    Rgb(up(c.0), up(c.1), up(c.2))
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
