use super::*;

#[test]
fn test_a_news_record_as_an_entry() {
    let (id, e) = news_entry("2026-11-13T01:40:13-08:00 [c3] BP-120 accepted: bullpen · BP-120 approved").unwrap();
    assert_eq!(id, "BP-120");
    assert_eq!(
        (e["kind"].as_str(), e["text"].as_str()),
        (Some("accepted"), Some("BP-120 approved"))
    );
    let (id, _) = news_entry("2026-11-13T01:40:13-08:00 BP-7 started: bullpen · BP-7 started").unwrap();
    assert_eq!(id, "BP-7");
    assert!(news_entry("not a record").is_none());
    assert!(news_entry("2026-11-13T01:40:13-08:00 BP-61 not sent: x").is_none());
}
