use super::*;

#[test]
fn the_standard_vectors() {
    assert_eq!(hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(hex(&vec![b'a'; 1_000_000]), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
}

#[test]
fn lengths_around_a_block_edge() {
    // what hashlib.sha1(b"x" * n).hexdigest() gives
    for (n, want) in [
        (55, "cef734ba81a024479e09eb5a75b6ddae62e6abf1"),
        (56, "901305367c259952f4e7af8323f480d59f81335b"),
        (63, "0ddc4e0cccd9a12850deb5abb0853a4425559fec"),
        (64, "bb2fa3ee7afb9f54c6dfb5d021f14b1ffe40c163"),
        (65, "78c741ddc482e4cdf8c474a0876347a0905b6233"),
        (119, "4300320394f7ee239bcdce7d3b8bcee173a0cd5c"),
        (120, "ceb2821639c4b6dcb10bce0e522ca2e608ce056d"),
        (128, "150fa3fbdc899bd0b8f95a9fb6027f564d953762"),
    ] {
        assert_eq!(hex(&vec![b'x'; n]), want, "{n} bytes");
    }
}
