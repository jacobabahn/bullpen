use super::html;

#[test]
fn paragraphs_are_split_by_blank_lines() {
    assert_eq!(html("One line\nand its next.\n\nAnother."), "<p>One line\nand its next.</p><p>Another.</p>");
    assert_eq!(html(""), "");
}

#[test]
fn emphasis_strong_and_both() {
    assert_eq!(html("*a* and _b_"), "<p><em>a</em> and <em>b</em></p>");
    assert_eq!(html("**a** and __b__"), "<p><strong>a</strong> and <strong>b</strong></p>");
    assert_eq!(html("***a***"), "<p><strong><em>a</em></strong></p>");
    assert_eq!(html("**bold with *em* in it**"), "<p><strong>bold with <em>em</em> in it</strong></p>");
}

#[test]
fn what_isnt_emphasis_stays_text() {
    assert_eq!(html("2 * 3 * 4"), "<p>2 * 3 * 4</p>");
    assert_eq!(html("snake_case_name and __init__"), "<p>snake_case_name and <strong>init</strong></p>");
    assert_eq!(html("a lone * star"), "<p>a lone * star</p>");
    assert_eq!(html(r"\*not em\*"), "<p>*not em*</p>");
}

#[test]
fn inline_code_keeps_its_text_as_it_is() {
    assert_eq!(html("run `bullpen ls`"), "<p>run <code>bullpen ls</code></p>");
    assert_eq!(html("`*not em*` and `<b>`"), "<p><code>*not em*</code> and <code>&lt;b&gt;</code></p>");
    assert_eq!(html("`` a ` b ``"), "<p><code>a ` b</code></p>");
    assert_eq!(html("an `unclosed span"), "<p>an `unclosed span</p>");
    assert_eq!(html("*see `a*b`*"), "<p><em>see <code>a*b</code></em></p>");
}

#[test]
fn fenced_code_is_one_block_escaped() {
    let md = "Before:\n\n```rust\nfn main() {\n    println!(\"<hi>\");\n}\n```\n\nAfter.";
    assert_eq!(
        html(md),
        "<p>Before:</p><pre><code>fn main() {\n    println!(&quot;&lt;hi&gt;&quot;);\n}</code></pre><p>After.</p>"
    );
    assert_eq!(html("~~~\n**not bold**\n~~~"), "<pre><code>**not bold**</code></pre>");
    assert_eq!(html("```\nnever closed"), "<pre><code>never closed</code></pre>");
}

#[test]
fn lists_bulleted_numbered_and_nested() {
    assert_eq!(html("- one\n- two"), "<ul><li>one</li><li>two</li></ul>");
    assert_eq!(html("* a\n+ b"), "<ul><li>a</li><li>b</li></ul>");
    assert_eq!(html("1. one\n2. two"), "<ol><li>one</li><li>two</li></ol>");
    assert_eq!(html("3) three\n4) four"), "<ol start=\"3\"><li>three</li><li>four</li></ol>");
    assert_eq!(
        html("- outer\n  - inner **b**\n  - more\n- last"),
        "<ul><li>outer<ul><li>inner <strong>b</strong></li><li>more</li></ul></li><li>last</li></ul>"
    );
    assert_eq!(html("1. a\n   - b"), "<ol><li>a<ul><li>b</li></ul></li></ol>");
    assert_eq!(html("Text:\n- a\n- b"), "<p>Text:</p><ul><li>a</li><li>b</li></ul>");
    assert_eq!(html("- a\ncarried on\n\nafter"), "<ul><li>a\ncarried on</li></ul><p>after</p>");
}

#[test]
fn an_item_of_paragraphs_keeps_them_and_its_code() {
    assert_eq!(
        html("- first\n\n  second\n- next"),
        "<ul><li><p>first</p><p>second</p></li><li>next</li></ul>"
    );
    assert_eq!(
        html("- run:\n  ```\n  ls\n  ```"),
        "<ul><li>run:<pre><code>ls</code></pre></li></ul>"
    );
    assert_eq!(html("- a\n\n- b"), "<ul><li>a</li><li>b</li></ul>");
}

#[test]
fn tables_with_a_head_and_rows() {
    let md = "| Name | What |\n| --- | :---: |\n| `a` | **b** |\n| c |";
    assert_eq!(
        html(md),
        "<table><thead><tr><th>Name</th><th>What</th></tr></thead>\
         <tbody><tr><td><code>a</code></td><td><strong>b</strong></td></tr><tr><td>c</td><td></td></tr></tbody></table>"
    );
    assert_eq!(
        html("a | b\n-|-"),
        "<table><thead><tr><th>a</th><th>b</th></tr></thead></table>"
    );
    assert_eq!(html(r"| a \| b |"), "<p>| a | b |</p>");
    assert_eq!(html("a | b\n---"), "<p>a | b</p>");
}

#[test]
fn links_to_web_pages() {
    assert_eq!(
        html("[the PR](https://github.com/o/r/pull/1)"),
        "<p><a href=\"https://github.com/o/r/pull/1\" rel=\"noopener noreferrer\" target=\"_blank\">the PR</a></p>"
    );
    assert_eq!(
        html("[a *b*](http://x.example/a_(b) \"title\")"),
        "<p><a href=\"http://x.example/a_(b)\" rel=\"noopener noreferrer\" target=\"_blank\">a <em>b</em></a></p>"
    );
    assert_eq!(
        html("see <https://x.example/?a=1&b=2>"),
        "<p>see <a href=\"https://x.example/?a=1&amp;b=2\" rel=\"noopener noreferrer\" target=\"_blank\">\
         https://x.example/?a=1&amp;b=2</a></p>"
    );
    assert_eq!(
        html("at https://x.example/p (and https://y.example/q)."),
        "<p>at <a href=\"https://x.example/p\" rel=\"noopener noreferrer\" target=\"_blank\">https://x.example/p</a> \
         (and <a href=\"https://y.example/q\" rel=\"noopener noreferrer\" target=\"_blank\">https://y.example/q</a>).</p>"
    );
    assert_eq!(html("[x](/api/fleet) and [y](#top)"), "<p>x and y</p>");
}

#[test]
fn a_heading_is_a_paragraph_in_bold_and_a_rule_is_nothing() {
    assert_eq!(html("## What changed ##\nText"), "<p><strong>What changed</strong></p><p>Text</p>");
    assert_eq!(html("#hashtag"), "<p>#hashtag</p>");
    assert_eq!(html("a\n\n---\n\nb"), "<p>a</p><p>b</p>");
}

// ---- hostile text comes out inert ----

/// No tag in the HTML but those it makes, and no attribute but theirs.
fn inert(out: &str) {
    let allowed = [
        "p", "/p", "em", "/em", "strong", "/strong", "code", "/code", "pre", "/pre", "ul", "/ul", "ol", "/ol", "li",
        "/li", "table", "/table", "thead", "/thead", "tbody", "/tbody", "tr", "/tr", "th", "/th", "td", "/td", "a",
        "/a",
    ];
    for tag in out.split('<').skip(1) {
        let tag = &tag[..tag.find('>').expect("a tag that closes")];
        let name = tag.split(' ').next().unwrap();
        assert!(allowed.contains(&name), "<{tag}> in {out}");
        if name == "a" {
            assert!(tag.starts_with("a href=\"http"), "<{tag}> in {out}");
            assert!(tag.ends_with(" rel=\"noopener noreferrer\" target=\"_blank\""), "<{tag}> in {out}");
        } else {
            assert!(name == tag || tag.starts_with("ol start=\""), "<{tag}> has attributes in {out}");
        }
    }
}

#[test]
fn a_script_is_text() {
    let out = html("<script>alert(1)</script>");
    assert_eq!(out, "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>");
    inert(&out);
    inert(&html("- <script>x</script>\n\n| <script> |\n|-|\n| </script> |"));
    inert(&html("```\n</code></pre><script>x</script>\n```"));
}

#[test]
fn a_javascript_link_is_its_text_alone() {
    for md in [
        "[click](javascript:alert(1))",
        "[click](JavaScript:alert(1))",
        "[click]( javascript:alert(1) )",
        "[click](data:text/html,<script>x</script>)",
        "<javascript:alert(1)>",
        "[click](https://x.example\"onmouseover=\"alert(1))",
    ] {
        let out = html(md);
        inert(&out);
        assert!(!out.contains("href=\"javascript"), "{md}: {out}");
        assert!(!out.contains("\"onmouseover"), "{md}: {out}");
    }
    assert_eq!(html("[click](javascript:alert(1))"), "<p>click</p>");
}

#[test]
fn an_image_is_its_alt_text_and_nothing_is_fetched() {
    let out = html("![a cat](https://x.example/cat.png) and ![<b>x</b>](https://x.example/y.png)");
    assert_eq!(out, "<p>a cat and &lt;b&gt;x&lt;/b&gt;</p>");
    inert(&out);
    assert!(!out.contains("img"));
}

#[test]
fn raw_html_and_attributes_are_text() {
    let out = html("<img src=x onerror=alert(1)> <a href=\"javascript:x\">y</a> <div onclick='z'>w</div>");
    inert(&out);
    assert!(out.starts_with("<p>&lt;img src=x onerror=alert(1)&gt; &lt;a href=&quot;javascript:x&quot;&gt;"));
    let out = html("**<b onmouseover=\"alert(1)\">bold</b>**");
    assert_eq!(out, "<p><strong>&lt;b onmouseover=&quot;alert(1)&quot;&gt;bold&lt;/b&gt;</strong></p>");
    inert(&html("[<img src=x onerror=alert(1)>](https://x.example) &lt;b&gt; &#60;script&#62;"));
    assert_eq!(html("&lt;script&gt;"), "<p>&amp;lt;script&amp;gt;</p>");
}
