use super::*;
use serde_json::json;

fn gate() -> Gate {
    Gate {
        users: vec!["ann@example.com".into()],
        host: "box.tail1.ts.net".into(),
        log: PathBuf::from("/nonexistent/web.log"),
    }
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.insert(*k, HeaderValue::from_str(v).unwrap());
    }
    h
}

const OK: [(&str, &str); 2] = [(LOGIN, "ann@example.com"), ("host", "box.tail1.ts.net")];

#[test]
fn a_get_needs_a_login_it_names_and_its_host() {
    let g = gate();
    assert_eq!(g.refusal(&Method::GET, &headers(&OK)), None);
    assert_eq!(
        g.refusal(
            &Method::GET,
            &headers(&[(LOGIN, "ANN@example.com"), ("host", "Box.tail1.ts.net:443")])
        ),
        None
    );
    assert!(g
        .refusal(&Method::GET, &headers(&OK[1..]))
        .unwrap()
        .contains("no Tailscale login"));
    let other = g
        .refusal(&Method::GET, &headers(&[(LOGIN, "bob@example.com"), OK[1]]))
        .unwrap();
    assert!(other.contains("bob@example.com isn't one of web_users"), "{other}");
    let host = g
        .refusal(&Method::GET, &headers(&[OK[0], ("host", "evil.example")]))
        .unwrap();
    assert!(host.contains("\"evil.example\" isn't box.tail1.ts.net"), "{host}");
}

#[test]
fn a_post_must_be_the_pages_own() {
    let g = gate();
    let mut ok = OK.to_vec();
    ok.extend([
        ("content-type", "application/json"),
        ("x-bullpen", "1"),
        ("sec-fetch-site", "same-origin"),
    ]);
    assert_eq!(g.refusal(&Method::POST, &headers(&ok)), None);
    let without = |name: &str| ok.iter().copied().filter(|(k, _)| *k != name).collect::<Vec<_>>();
    assert!(g.refusal(&Method::POST, &headers(&without("x-bullpen"))).is_some());
    assert!(g.refusal(&Method::POST, &headers(&without("content-type"))).is_some());
    assert!(g.refusal(&Method::POST, &headers(&without("sec-fetch-site"))).is_some());
    // an Origin that is the page's does in place of Sec-Fetch-Site
    let mut origin = without("sec-fetch-site");
    origin.push(("origin", "https://box.tail1.ts.net"));
    assert_eq!(g.refusal(&Method::POST, &headers(&origin)), None);
    let mut cross = without("sec-fetch-site");
    cross.extend([("sec-fetch-site", "cross-site"), ("origin", "https://evil.example")]);
    assert!(g
        .refusal(&Method::POST, &headers(&cross))
        .unwrap()
        .contains("another site"));
}

#[test]
fn tailscale_status_names_this_machines_login_and_host() {
    let status = json!({
        "Self": {"UserID": 42, "DNSName": "Box.tail1.ts.net."},
        "User": {"42": {"LoginName": "ann@example.com"}, "7": {"LoginName": "bob@example.com"}},
    });
    let me = me_in(&status).unwrap();
    assert_eq!(
        (me.login.as_str(), me.host.as_str()),
        ("ann@example.com", "box.tail1.ts.net")
    );
    assert!(me_in(&json!({"Self": {"UserID": 1, "DNSName": "x"}, "User": {}})).is_none());
}

#[test]
fn a_log_line_is_one_line() {
    assert_eq!(printable("a\nb\x1b[31m\n"), "a?b?[31m\n");
}

#[test]
fn web_users_is_a_list_of_logins() {
    assert_eq!(list(" ann@x.com, bob@y.com ,,"), ["ann@x.com", "bob@y.com"]);
    assert!(list("").is_empty());
}
