//! Writing an image as PNG (8-bit RGBA, straight alpha), for the terminal and for
//! `--render`: each row filtered the way that leaves it smallest (none, against the
//! pixel to its left, or against the row above), then zlib (miniz_oxide).

use tiny_skia::Pixmap;

/// The image as PNG bytes.
pub fn encode(pm: &Pixmap) -> Vec<u8> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let stride = w * 4;
    let mut raw = vec![0; (stride + 1) * h];
    for y in 0..h {
        let row = &rgba[y * stride..(y + 1) * stride];
        let above = (y > 0).then(|| &rgba[(y - 1) * stride..y * stride]);
        filter(row, above, &mut raw[y * (stride + 1)..(y + 1) * (stride + 1)]);
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

/// A row filtered into `into` (the filter's number, then the bytes): by Sub (each byte
/// less the pixel's to its left) or Up (less the byte above), whichever leaves the
/// smaller bytes, or not at all.
fn filter(row: &[u8], above: Option<&[u8]>, into: &mut [u8]) {
    let small = |b: u8| (b as i8).unsigned_abs() as u32;
    let sub = |i: usize| row[i].wrapping_sub(if i >= 4 { row[i - 4] } else { 0 });
    let none_cost: u32 = row.iter().map(|&b| small(b)).sum();
    let sub_cost: u32 = (0..row.len()).map(|i| small(sub(i))).sum();
    let up_cost = above.map(|a| row.iter().zip(a).map(|(x, y)| small(x.wrapping_sub(*y))).sum::<u32>());
    let (kind, bytes) = into.split_at_mut(1);
    if up_cost.is_some_and(|u| u < sub_cost.min(none_cost)) {
        kind[0] = 2;
        let a = above.expect("a row above");
        bytes.iter_mut().zip(row.iter().zip(a)).for_each(|(o, (x, y))| *o = x.wrapping_sub(*y));
    } else if sub_cost < none_cost {
        kind[0] = 1;
        bytes.iter_mut().enumerate().for_each(|(i, o)| *o = sub(i));
    } else {
        kind[0] = 0;
        bytes.copy_from_slice(row);
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// PNG's checksum (CRC-32, the polynomial zlib uses), a byte at a time from a table.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    !data.iter().fold(!0u32, |crc, &b| table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8))
}

#[cfg(test)]
#[path = "tests/test_png.rs"]
mod tests;
