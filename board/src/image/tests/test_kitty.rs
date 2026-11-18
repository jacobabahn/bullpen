use super::*;

#[test]
fn the_query_asks_about_a_one_pixel_image_it_doesnt_keep() {
    assert_eq!(query(7), "\x1b_Gi=7,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
}

#[test]
fn an_upload_is_png_in_base64_chunks_the_last_saying_so() {
    let small = upload(5, b"abc");
    assert_eq!(small, "\x1b_Ga=t,f=100,t=d,i=5,q=2,m=0;YWJj\x1b\\");
    let big = upload(5, &vec![0u8; 6000]); // 8000 base64 bytes: two chunks
    let parts: Vec<&str> = big.split("\x1b\\").filter(|p| !p.is_empty()).collect();
    assert_eq!(parts.len(), 2);
    assert!(parts[0].starts_with("\x1b_Ga=t,f=100,t=d,i=5,q=2,m=1;"));
    assert!(parts[1].starts_with("\x1b_Gq=2,m=0;"));
    assert_eq!(parts[0].len() - "\x1b_Ga=t,f=100,t=d,i=5,q=2,m=1;".len(), 4096);
}

#[test]
fn a_placement_crops_the_image_to_its_cells_and_puts_the_cursor_back() {
    let p = Place { id: 9, p: 1, col: 0, row: 2, crop_x: 0, crop_y: 60, crop_w: 400, crop_h: 200, cols: 40, rows: 10, z: 0 };
    assert_eq!(place(&p), "\x1b7\x1b[3;1H\x1b_Ga=p,i=9,p=1,x=0,y=60,w=400,h=200,c=40,r=10,C=1,q=2\x1b\\\x1b8");
    // another placement of it, over the images at z 0
    let q = Place { p: 3, col: 39, crop_x: 10, z: 1, ..p };
    assert_eq!(place(&q), "\x1b7\x1b[3;40H\x1b_Ga=p,i=9,p=3,x=10,y=60,w=400,h=200,c=40,r=10,z=1,C=1,q=2\x1b\\\x1b8");
}

#[test]
fn hiding_keeps_the_image_and_deleting_frees_it() {
    assert_eq!(hide(9), "\x1b_Ga=d,d=i,i=9,q=2\x1b\\");
    assert_eq!(unplace(9, 3), "\x1b_Ga=d,d=i,i=9,p=3,q=2\x1b\\");
    assert_eq!(delete(9), "\x1b_Ga=d,d=I,i=9,q=2\x1b\\");
}

#[test]
fn base64_is_the_standard_one() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}
