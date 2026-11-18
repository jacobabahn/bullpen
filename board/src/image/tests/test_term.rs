use super::*;
use crate::image::theme::Rgb;
use std::time::Duration;

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
fn answers_cut_off_wait_for_the_rest_and_are_never_keys() {
    let a = parse(b"\x1b_Gi=5;OK\x1b\\\x1b]10;rgb:ff", Some(5));
    assert!(a.graphics && !a.done);
    assert!(a.rest.is_empty() && a.whole == 11);
    // an OSC answer cut anywhere gives no keys, a lone esc at the end included
    let osc = b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\";
    for cut in 1..osc.len() {
        let a = parse(&osc[..cut], Some(5));
        assert!(keys(&a.rest).is_empty(), "{cut}");
    }
}

/// Input as a terminal sends it, in pieces; then nothing.
struct Script(Vec<&'static [u8]>);

impl Input for Script {
    fn next(&mut self, _wait: Duration) -> Option<Vec<u8>> {
        (!self.0.is_empty()).then(|| self.0.remove(0).to_vec())
    }
}

#[test]
fn an_answer_in_pieces_is_read_whole() {
    let mut input = Script(vec![b"\x1b_Gi=5;O", b"K\x1b\\j\x1b]10;rgb:ff", b"ff/0000/0000\x1b\\", DA, b"k"]);
    let (a, left) = read_answers(&mut input, Some(5));
    assert!(a.graphics && a.done && left.is_empty());
    assert_eq!(a.said.fg, Some(Rgb(255, 0, 0)));
    assert_eq!(keys(&a.rest), keys(b"j"));
}

#[test]
fn answers_that_come_late_are_read_to_their_end_and_only_the_keys_passed_on() {
    // the time runs out mid-answer (the script stops), leaving it cut off
    let mut input = Script(vec![b"r\x1b]11;rgb:1e1e/"]);
    let (a, left) = read_answers(&mut input, Some(5));
    assert!(!a.done);
    assert_eq!(keys(&a.rest), keys(b"r")); // the r typed before it
    assert_eq!(left, b"\x1b]11;rgb:1e1e/");
    // the rest comes late, with a key typed meanwhile: only the key is passed on
    let mut late = Script(vec![b"1e1e/2e2e\x1b", b"\\g\x1b[?62c\x1b"]);
    let mut typed = Vec::new();
    drain(&mut late, left, Some(5), |t| typed.push(t));
    let esc = Typed::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(typed, [keys(b"g"), vec![esc]].concat());
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
fn alt_with_a_key_delete_and_modified_arrows_are_passed_on() {
    let k = |c, m| Typed::Key(KeyEvent::new(c, m));
    let (none, alt) = (KeyModifiers::NONE, KeyModifiers::ALT);
    assert_eq!(
        keys(b"\x1bb\x1bf\x1b\x7f\x1b[3~\x1b[1;3D\x1b[1;5C\x1b[Z\x1b\x1b"),
        vec![
            k(KeyCode::Char('b'), alt),
            k(KeyCode::Char('f'), alt),
            k(KeyCode::Backspace, alt),
            k(KeyCode::Delete, none),
            k(KeyCode::Left, alt),
            k(KeyCode::Right, KeyModifiers::CONTROL),
            k(KeyCode::BackTab, none),
            k(KeyCode::Esc, none),
            k(KeyCode::Esc, none),
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
