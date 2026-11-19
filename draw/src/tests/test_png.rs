use super::*;
use tiny_skia::Color;

#[test]
fn an_image_is_written_as_png_and_reads_back_the_same() {
    let mut pm = Pixmap::new(3, 2).unwrap();
    pm.fill(Color::from_rgba8(10, 20, 30, 255));
    let png = encode(&pm);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 2);
    assert!(png.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
    // the IDAT's rows, unfiltered, are the pixels
    let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    assert_eq!(&png[37..41], b"IDAT");
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + idat_len]).unwrap();
    assert_eq!(raw.len(), 2 * (1 + 3 * 4));
    // the first row by Sub (the pixel, then each less the one before), the second by Up
    assert_eq!(&raw[..9], &[1, 10, 20, 30, 255, 0, 0, 0, 0]);
    assert_eq!(&raw[13..18], &[2, 0, 0, 0, 0]);
}

#[test]
fn the_checksum_is_pngs() {
    assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn a_translucent_pixel_is_written_straight_as_tiny_skia_makes_it() {
    let mut pm = Pixmap::new(256, 1).unwrap();
    for (x, px) in pm.pixels_mut().iter_mut().enumerate() {
        let a = x as u8;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(a / 3, a / 2, a, a).unwrap();
    }
    let png = encode(&pm);
    let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + idat_len]).unwrap();
    // one row: undo its filter
    let mut row = raw[1..].to_vec();
    if raw[0] == 1 {
        for i in 4..row.len() {
            row[i] = row[i].wrapping_add(row[i - 4]);
        }
    }
    for (x, px) in pm.pixels().iter().enumerate() {
        let c = if px.alpha() == 0 { tiny_skia::ColorU8::from_rgba(0, 0, 0, 0) } else { px.demultiply() };
        assert_eq!(&row[x * 4..x * 4 + 4], &[c.red(), c.green(), c.blue(), c.alpha()], "alpha {x}");
    }
}
