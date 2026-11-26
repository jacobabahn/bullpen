//! The image look's colours: the terminal's own (its foreground, background and the
//! palette's red, green, yellow and blue, asked by OSC 10, 11 and 4: answers.rs), so an
//! image follows the Rex theme; the colours the terminal doesn't say are xterm's. The
//! tones drawn with them (dim text, a faint fill, a rule, a keycap) and what each status
//! and risk's colour is (the board's meanings: amber needs you, red stopped or high risk,
//! green ready or low risk, blue in review). The board's colours for a flag are its own
//! (board/src/image/theme.rs); the tour's for code are cli/src/stepper/look/.

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

    /// Dim text: the foreground at 70% (76% on a light theme, where thin grey type on
    /// white goes faint), which reads at over 4.5:1 on a light theme's white, even on
    /// the selected card (and at over 6:1 on a dark one).
    pub fn dim(&self) -> [u8; 4] {
        faint(self.fg, if luminance(self.bg) > luminance(self.fg) { 0.76 } else { 0.7 })
    }

    /// A group's name: the foreground, a little dim, so the titles under it lead.
    pub fn heading(&self) -> [u8; 4] {
        faint(self.fg, 0.78)
    }

    /// The title of a task that isn't waiting on you (working, in review, queued): the
    /// foreground a step back, so the titles that are yours lead the list.
    pub fn soft(&self) -> [u8; 4] {
        faint(self.fg, 0.86)
    }

    /// A quiet title (the coordinator's task, a closed one): dimmer than the rest, a
    /// step above its own lines.
    pub fn quiet(&self) -> [u8; 4] {
        faint(self.fg, 0.8)
    }

    /// The selected card's fill: the terminal's blue toward the foreground, faintly (a
    /// little more on a dark pane, where a faint fill lifts less), so the card reads as
    /// chosen, as its edge's colour, not as a grey, disabled one.
    pub fn tint(&self) -> [u8; 4] {
        let mix = |b: u8, f: u8| ((b as u16 * 2 + f as u16) / 3) as u8;
        let c = Rgb(mix(self.blue.0, self.fg.0), mix(self.blue.1, self.fg.1), mix(self.blue.2, self.fg.2));
        faint(c, if luminance(self.fg) > luminance(self.bg) { 0.2 } else { 0.12 })
    }

    /// A working mark's ring: the foreground, a little stronger than the tint (more
    /// on a light pane, where a faint grey ring all but goes).
    pub fn tint_strong(&self) -> [u8; 4] {
        faint(self.fg, if luminance(self.fg) > luminance(self.bg) { 0.25 } else { 0.34 })
    }

    /// The selected card's edge: the terminal's blue, a little clear, so it's mixed into
    /// the card's own fill. Nothing else on the board is boxed but a selected task that
    /// needs you (edged in its own colour), so the ring is the selection wherever it is.
    pub fn edge(&self) -> [u8; 4] {
        faint(self.blue, EDGE)
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

    /// A risk's chip's line: the foreground, a little stronger than a keycap's edge, as
    /// the chip has no fill.
    pub fn chip_edge(&self) -> [u8; 4] {
        faint(self.fg, 0.38)
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
            "ready" => Some(solid(self.green)),
            // (its own teal: the bar and its count tell it from what's ready)
            "finished" => Some(solid(self.teal())),
            "review" => Some(solid(self.blue)),
            _ => None,
        }
    }

    /// A group's colour in the header's status bar, by its first status: what waits on
    /// you in its status's colour (finished its teal), in review blue, the
    /// coordinator's a violet of its own, and what's working or queued in the
    /// foreground, the queued fainter.
    pub fn group(&self, status: &str) -> [u8; 4] {
        match status {
            "needs" | "stopped" | "ready" | "finished" | "review" => self.shape(status).unwrap_or(self.text()),
            // (quieter than the colours: what doesn't wait on you doesn't outshine them)
            "working" => faint(self.fg, 0.6),
            "triage" => solid(self.between()),
            _ => faint(self.fg, 0.42),
        }
    }

    /// Amber a quarter of the way to red, at its brightest: what's yours as a fill (a
    /// faint yellow on a dark pane is olive; this stays warm). The board washes a task
    /// that needs you with it, the tour your comments.
    pub fn amber(&self) -> Rgb {
        let warm = |y: u8, r: u8| ((y as u16 * 3 + r as u16) / 4) as u8;
        bright(Rgb(warm(self.yellow.0, self.red.0), warm(self.yellow.1, self.red.1), warm(self.yellow.2, self.red.2)))
    }

    /// A teal, the terminal's green half way to its blue: a finished task's, in its
    /// mark, its status bar segment and its count, apart from a ready one's green.
    pub fn teal(&self) -> Rgb {
        let mix = |g: u8, b: u8| ((g as u16 + b as u16) / 2) as u8;
        Rgb(mix(self.green.0, self.blue.0), mix(self.green.1, self.blue.1), mix(self.green.2, self.blue.2))
    }

    /// A violet between the terminal's red and blue, toward the foreground: the
    /// coordinator's tasks in the status bar, which are neither yours nor a worker's.
    pub fn between(&self) -> Rgb {
        let mix = |a: u8, b: u8, c: u8| ((a as u16 * 3 + b as u16 * 4 + c as u16) / 8) as u8;
        Rgb(mix(self.red.0, self.blue.0, self.fg.0), mix(self.red.1, self.blue.1, self.fg.1), mix(self.red.2, self.blue.2, self.fg.2))
    }

    /// A solid pill of a colour (an overdue age's chip) and what's written on it: a
    /// light colour (amber) at its brightest, with the darker of the pane's foreground
    /// and background on it; a dark one (red) with the lighter on it, the colour taken
    /// toward black until that reads at 4:1 (its word is semibold and short).
    pub fn pill(&self, c: Rgb) -> ([u8; 4], [u8; 4]) {
        let (light, dark) = if luminance(self.fg) > luminance(self.bg) { (self.fg, self.bg) } else { (self.bg, self.fg) };
        let lit = bright(c);
        if luminance(lit) > 0.4 {
            return (solid(lit), solid(dark));
        }
        let apart = |a: Rgb, b: Rgb| (luminance(a).max(luminance(b)) + 0.05) / (luminance(a).min(luminance(b)) + 0.05);
        let mut fill = c;
        for _ in 0..20 {
            if apart(fill, light) >= 4.0 {
                break;
            }
            let down = |v: u8| (v as u16 * 19 / 20) as u8;
            fill = Rgb(down(fill.0), down(fill.1), down(fill.2));
        }
        (solid(fill), solid(light))
    }

    /// A chip's word in a colour, on that colour faintly: on a light theme the colour
    /// half way to the foreground, so it reads on its pale fill as text does.
    pub fn word(&self, c: [u8; 4]) -> [u8; 4] {
        if luminance(self.bg) <= luminance(self.fg) {
            return c;
        }
        let mix = |a: u8, b: u8| ((a as u16 + b as u16) / 2) as u8;
        [mix(c[0], self.fg.0), mix(c[1], self.fg.1), mix(c[2], self.fg.2), c[3]]
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

/// A colour's relative luminance, as WCAG works it out (0 black, 1 white).
pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// How much of the selected card's edge is the terminal's blue.
const EDGE: f32 = 0.85;

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
