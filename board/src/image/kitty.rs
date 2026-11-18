//! The kitty graphics protocol's commands, as bytes: the query that says whether the
//! terminal draws images, an upload (PNG, base64 in chunks), a placement (a crop of
//! the image pinned to a block of cells: the terminal scales the crop to them; an
//! image can have several, each with its number, and one with a z-index above another's),
//! and taking one placement or all of an image off the screen, or deleting it. Every command but the query asks for no answer (q=2), so none
//! comes back as keys.

/// Base64 chunks the protocol takes at most.
const CHUNK: usize = 4096;

fn apc(control: &str, payload: &str) -> String {
    if payload.is_empty() {
        format!("\x1b_G{control}\x1b\\")
    } else {
        format!("\x1b_G{control};{payload}\x1b\\")
    }
}

/// Ask whether the terminal draws images: a one-pixel image it checks and doesn't keep.
/// A terminal that does answers `ESC _ G i=<id>;OK ESC \`.
pub fn query(id: u32) -> String {
    apc(&format!("i={id},s=1,v=1,a=q,t=d,f=24"), "AAAA")
}

/// Send a PNG as image `id`, in chunks.
pub fn upload(id: u32, png: &[u8]) -> String {
    let data = base64(png);
    let chunks: Vec<&str> = if data.is_empty() {
        vec![""]
    } else {
        data.as_bytes().chunks(CHUNK).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect()
    };
    let mut out = String::with_capacity(data.len() + chunks.len() * 32);
    for (k, chunk) in chunks.iter().enumerate() {
        let more = u8::from(k + 1 < chunks.len());
        // q=2 on each chunk: kitty takes it from the first, another terminal may not
        let control = if k == 0 { format!("a=t,f=100,t=d,i={id},q=2,m={more}") } else { format!("q=2,m={more}") };
        out.push_str(&apc(&control, chunk));
    }
    out
}

/// Where an image is shown: its placement's number (a new placement with the same
/// number replaces it in place), the top-left cell (0-based), the crop of the image (in
/// pixels), the block of cells it fills, and its z-index (over the images at 0 when 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Place {
    pub id: u32,
    pub p: u32,
    pub col: u16,
    pub row: u16,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_w: u32,
    pub crop_h: u32,
    pub cols: u16,
    pub rows: u16,
    pub z: i32,
}

/// Show (or move) an image's placement: the cursor goes to its cell and back, and
/// doesn't move for the image (C=1).
pub fn place(p: &Place) -> String {
    let z = if p.z == 0 { String::new() } else { format!(",z={}", p.z) };
    let control = format!(
        "a=p,i={},p={},x={},y={},w={},h={},c={},r={}{z},C=1,q=2",
        p.id, p.p, p.crop_x, p.crop_y, p.crop_w, p.crop_h, p.cols, p.rows
    );
    format!("\x1b7\x1b[{};{}H{}\x1b8", p.row + 1, p.col + 1, apc(&control, ""))
}

/// Take image `id` off the screen, keeping its data to place again.
pub fn hide(id: u32) -> String {
    apc(&format!("a=d,d=i,i={id},q=2"), "")
}

/// Take placement `p` of image `id` off the screen, keeping the image and its others.
pub fn unplace(id: u32, p: u32) -> String {
    apc(&format!("a=d,d=i,i={id},p={p},q=2"), "")
}

/// Delete image `id` and its data.
pub fn delete(id: u32) -> String {
    apc(&format!("a=d,d=I,i={id},q=2"), "")
}

pub fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(ABC[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/test_kitty.rs"]
mod tests;
