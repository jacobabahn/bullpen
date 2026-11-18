use super::*;
use crate::image::theme::Rgb;

const DA: &[u8] = b"\x1b[?62;22c";

#[test]
fn the_questions_end_with_the_device_attributes() {
    let q = questions(Some(5));
    assert!(q.starts_with("\x1b_Gi=5,"));
    assert!(q.contains("\x1b]10;?\x1b\\") && q.contains("\x1b]11;?\x1b\\") && q.contains("\x1b]4;1;?;2;?;3;?;4;?\x1b\\"));
    assert!(q.ends_with("\x1b[16t\x1b[c"));
    assert!(!questions(None).contains("\x1b_G"));
}

#[test]
fn a_terminal_that_draws_images_says_ok_to_the_query_before_its_attributes() {
    let mut said = b"\x1b_Gi=5;OK\x1b\\\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:0000/0000/0000\x07".to_vec();
    said.extend(b"\x1b]4;1;rgb:cc/00/00\x1b\\\x1b]4;4;rgb:00/00/ee\x1b\\\x1b[6;34;16t");
    said.extend(DA);
    let a = parse(&said, Some(5));
    assert!(a.graphics && a.done);
    assert_eq!((a.said.fg, a.said.bg), (Some(Rgb(255, 255, 255)), Some(Rgb(0, 0, 0))));
    assert_eq!(a.said.palette, vec![(1, Rgb(0xcc, 0, 0)), (4, Rgb(0, 0, 0xee))]);
    assert_eq!(a.cell, Some((16, 34)));
    assert!(a.rest.is_empty());
}

#[test]
fn a_terminal_that_doesnt_answers_only_its_attributes() {
    let a = parse(DA, Some(5));
    assert!(!a.graphics && a.done);
    assert_eq!(a.said, Default::default());
    // an error, or another image's OK, isn't a yes
    let a = parse(b"\x1b_Gi=5;ENOTSUPPORTED:no\x1b\\\x1b[?1c", Some(5));
    assert!(!a.graphics && a.done);
    let a = parse(b"\x1b_Gi=6;OK\x1b\\\x1b[?1c", Some(5));
    assert!(!a.graphics);
}

#[test]
fn answers_cut_off_wait_for_the_rest() {
    let a = parse(b"\x1b_Gi=5;OK\x1b\\\x1b]10;rgb:ff", Some(5));
    assert!(a.graphics && !a.done);
}

#[test]
fn keys_typed_while_the_board_asked_are_passed_on() {
    let a = parse(b"j\x1b_Gi=5;OK\x1b\\k\x1b[A\x1b[?1c", Some(5));
    assert!(a.done);
    let keys = keys(&a.rest);
    let k = |c| Typed::Key(KeyEvent::new(c, KeyModifiers::NONE));
    assert_eq!(keys, vec![k(KeyCode::Char('j')), k(KeyCode::Char('k')), k(KeyCode::Up)]);
}

#[test]
fn the_keys_read_are_letters_enter_esc_ctrl_arrows_and_paste() {
    let typed = keys("é\r\x7f\x04\x1b\x1b[6~\x1b[200~a\nb\x1b[201~L\x1b[99X".as_bytes());
    let k = |c, m| Typed::Key(KeyEvent::new(c, m));
    let none = KeyModifiers::NONE;
    assert_eq!(
        typed,
        vec![
            k(KeyCode::Char('é'), none),
            k(KeyCode::Enter, none),
            k(KeyCode::Backspace, none),
            k(KeyCode::Char('d'), KeyModifiers::CONTROL),
            k(KeyCode::Esc, none),
            k(KeyCode::PageDown, none),
            Typed::Paste("a\nb".into()),
            k(KeyCode::Char('L'), KeyModifiers::SHIFT),
        ]
    );
}

#[test]
fn the_gate_hands_input_to_the_answers_and_back() {
    let gate = std::sync::Arc::new(Gate::default());
    assert!(!gate.wanted());
    assert!(gate.want());
    assert!(!gate.want()); // asking already
    assert!(gate.wanted());
    let reader = gate.clone();
    let t = std::thread::spawn(move || {
        reader.park(); // waits for the questions
        reader.done();
    });
    while gate.get() != Stage::Parked {
        std::thread::yield_now();
    }
    gate.asked();
    t.join().unwrap();
    assert_eq!(gate.get(), Stage::Keys);
}
