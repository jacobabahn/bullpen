//! What bullpen's image look draws with, apart from what it draws: the type (font.rs),
//! the shapes (shape.rs), the terminal's colours and their tones (theme.rs), PNG
//! (png.rs), the kitty graphics protocol's bytes (kitty.rs), cutting an image into
//! pieces under the terminal's limit (cut.rs) and the terminal's answers to the
//! questions asked before drawing (answers.rs). The board's image look
//! (board/src/image/) and the tour's (cli/src/stepper/look/) both draw with these.

pub mod answers;
pub mod cut;
pub mod font;
pub mod kitty;
pub mod png;
pub mod shape;
pub mod theme;
