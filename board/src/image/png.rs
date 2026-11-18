//! Writing an image as PNG (8-bit RGBA, straight alpha), for the terminal and for
//! `--render`: each row filtered the way that leaves it smallest (none, against the
//! pixel to its left, or against the row above), then zlib (miniz_oxide).

use tiny_skia::Pixmap;

/// The image as PNG bytes.
pub fn encode(pm: &Pixmap) -> Vec<u8> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let rgba: Vec<u8> = pm.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }).collect();
    let stride = w * 4;
    let mut raw = Vec::with_capacity((stride + 1) * h);
    for y in 0..h {
        let row = &rgba[y * stride..(y + 1) * stride];
        let above = if y == 0 { None } else { Some(&rgba[(y - 1) * stride..y * stride]) };
        let (kind, bytes) = filtered(row, above);
        raw.push(kind);
        raw.extend(bytes);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend((w as u32).to_be_bytes());
    ihdr.extend((h as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8 bits, RGBA, deflate, adaptive filters, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A row filtered: by Sub (each byte less the pixel's to its left) or Up (less the
/// byte above), whichever leaves the smaller bytes, or not at all; with the filter's number.
fn filtered(row: &[u8], above: Option<&[u8]>) -> (u8, Vec<u8>) {
    let sub: Vec<u8> = (0..row.len()).map(|i| row[i].wrapping_sub(if i >= 4 { row[i - 4] } else { 0 })).collect();
    let mut best = (0, row.to_vec());
    let cost = |b: &[u8]| b.iter().map(|&x| (x as i8).unsigned_abs() as u32).sum::<u32>();
    let mut least = cost(row);
    let mut consider = |kind: u8, bytes: Vec<u8>| {
        let c = cost(&bytes);
        if c < least {
            least = c;
            best = (kind, bytes);
        }
    };
    consider(1, sub);
    if let Some(above) = above {
        consider(2, row.iter().zip(above).map(|(a, b)| a.wrapping_sub(*b)).collect());
    }
    best
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// PNG's checksum (CRC-32, the polynomial zlib uses).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
#[path = "tests/test_png.rs"]
mod tests;
