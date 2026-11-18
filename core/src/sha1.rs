//! SHA-1 (FIPS 180-4), for a review's own lines (review.rs): hashlib.sha1's hex digest,
//! with no crate, so the hashes the Python wrote into a task still match.

const H0: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];

/// The digest of data, as 40 lowercase hex digits.
pub fn hex(data: &[u8]) -> String {
    digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The 20-byte digest of data.
pub fn digest(data: &[u8]) -> [u8; 20] {
    let mut h = H0;
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        compress(&mut h, block);
    }
    for block in padded_tail(rest, data.len()).as_chunks::<64>().0 {
        compress(&mut h, block);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The last partial block, then 0x80, zeros, and the length in bits: one or two blocks.
fn padded_tail(rest: &[u8], len: usize) -> Vec<u8> {
    let mut tail = rest.to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&((len as u64).wrapping_mul(8)).to_be_bytes());
    tail
}

/// One 64-byte block into the state.
fn compress(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*word);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5a827999),
            20..=39 => (b ^ c ^ d, 0x6ed9eba1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
            _ => (b ^ c ^ d, 0xca62c1d6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
        *x = x.wrapping_add(y);
    }
}

#[cfg(test)]
#[path = "tests/test_sha1.rs"]
mod tests;
