//! The image look's colours: the terminal's own (its foreground, background and the
//! palette's red, green, yellow and blue, asked by OSC 10, 11 and 4), so the image
//! follows the Rex theme; the colours the terminal doesn't say are xterm's. What each
//! colour means is the text board's (ui/mod.rs): amber needs you, red stopped or high
//! risk, green ready or low risk, blue in review. The image's background is clear, so
//! the pane's own shows through; dim text and the selected card are the foreground,
//! faint, and a task that needs you is washed with its colour. The colours and their
//! tones are bullpen-draw's (draw/src/theme.rs), which the tour draws with too; a flag's
//! colours are the board's own (`Flags`).

pub use bullpen_draw::theme::*;

use crate::ui::rows::Flag;

/// The colours of a task that needs you or stopped.
pub trait Flags {
    fn flag_tint(&self, flag: Flag) -> [u8; 4];
    fn flag(&self, flag: Flag) -> Rgb;
    fn wash(&self, flag: Flag) -> [u8; 4];
}

impl Flags for Theme {
    /// The selected card's fill when its task needs you or stopped: its flag's colour,
    /// over twice as strong as its wash (on a dark pane less, where amber goes brown),
    /// under an edge of that colour (render.rs's `card`).
    fn flag_tint(&self, flag: Flag) -> [u8; 4] {
        faint(fill(self, flag), if luminance(self.bg) > luminance(self.fg) { 0.22 } else { 0.17 })
    }

    /// A flag's colour: amber for a task that needs you, red for one whose agent stopped.
    fn flag(&self, flag: Flag) -> Rgb {
        match flag {
            Flag::Needs => self.yellow,
            Flag::Stopped => self.red,
        }
    }

    /// Behind a task that needs you or stopped: its flag's colour at its brightest (a
    /// light theme's amber is dark, to read as text, and a wash of it is beige),
    /// faintly, so dim text on it still reads at 4.5:1 on a light theme's white.
    fn wash(&self, flag: Flag) -> [u8; 4] {
        // (a stopped one's a little stronger on a dark pane, so it's red, not brown)
        let dark = luminance(self.bg) < luminance(self.fg);
        faint(fill(self, flag), if dark && flag == Flag::Stopped { 1.3 * WASH } else { WASH })
    }
}

/// How strong a wash is: faint, so the selected card (three times as strong) is told
/// from the washes round it.
const WASH: f32 = 0.07;

/// A flag's colour as a fill: amber a quarter of the way to red (a faint yellow on
/// a dark pane is olive; this stays warm), at its brightest.
fn fill(t: &Theme, flag: Flag) -> Rgb {
    match flag {
        Flag::Needs => t.amber(),
        Flag::Stopped => bright(t.red),
    }
}

#[cfg(test)]
#[path = "tests/test_theme.rs"]
mod tests;
