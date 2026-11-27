//! What an agent said, from Markdown into HTML for the page (docs/prd/web.md,
//! "Transcripts into chat"): paragraphs, emphasis, inline and fenced code, lists,
//! tables and links (`http` and `https` only), and nothing else. Every character of the
//! text goes out escaped: the only tags in the HTML are those, made here, and the only
//! attributes a link's own (`href`, `rel`, `target`) and an ordered list's `start`. A
//! heading is a paragraph in bold, an image its alt text, a link to anything but a web
//! page its text alone, and raw HTML is text.

/// The Markdown as HTML.
pub fn html(md: &str) -> String {
    let lines: Vec<String> = md.lines().map(expand_tabs).collect();
    let mut out = String::new();
    blocks(&lines, false, &mut out);
    out
}

/// A line's leading tabs as four spaces each.
fn expand_tabs(line: &str) -> String {
    let tabs = line.len() - line.trim_start_matches('\t').len();
    format!("{}{}", "    ".repeat(tabs), &line[tabs..])
}

// ---- blocks ----

/// The blocks of these lines: a tight list item's paragraphs go out bare.
fn blocks(lines: &[String], tight: bool, out: &mut String) {
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if blank(line) || rule(line) {
            i += 1;
        } else if let Some(f) = fence(line) {
            i = code(lines, i, f, out);
        } else if let Some(m) = marker(line) {
            i = list(lines, i, m, out);
        } else if table_at(lines, i) {
            i = table(lines, i, out);
        } else if let Some(text) = heading(line) {
            out.push_str(&format!("<p><strong>{}</strong></p>", inline(text)));
            i += 1;
        } else {
            let start = i;
            i += 1;
            while i < lines.len() && !interrupts(lines, i) {
                i += 1;
            }
            let text: Vec<&str> = lines[start..i].iter().map(|l| l.trim()).collect();
            let text = inline(&text.join("\n"));
            if tight {
                out.push_str(&text);
            } else {
                out.push_str(&format!("<p>{text}</p>"));
            }
        }
    }
}

/// Whether the line at i ends a paragraph: a blank line, or another block's start.
fn interrupts(lines: &[String], i: usize) -> bool {
    let l = &lines[i];
    blank(l) || rule(l) || fence(l).is_some() || marker(l).is_some() || heading(l).is_some() || table_at(lines, i)
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A thematic break (`---`, `***`, `___`), or a setext heading's underline (`===`):
/// nothing on the page, the paragraphs' gap is enough.
fn rule(line: &str) -> bool {
    let t: String = line.trim().chars().filter(|c| *c != ' ').collect();
    let first = t.chars().next();
    t.len() >= 3 && matches!(first, Some('-' | '*' | '_' | '=')) && t.chars().all(|x| Some(x) == first)
}

/// `#` to `######` and a space: the heading's text, its closing `#`s taken off.
fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let hashes = t.len() - t.trim_start_matches('#').len();
    let rest = &t[hashes..];
    if !(1..=6).contains(&hashes) || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim_end())
}

/// A code fence's opening: its character, how many, and how far it's indented.
#[derive(Clone, Copy)]
struct Fence {
    ch: char,
    len: usize,
    indent: usize,
}

fn fence(line: &str) -> Option<Fence> {
    let indent = indent(line);
    let t = &line[indent..];
    let ch = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = t.len() - t.trim_start_matches(ch).len();
    let info = &t[len..];
    (len >= 3 && !(ch == '`' && info.contains('`'))).then_some(Fence { ch, len, indent })
}

/// A fenced block from its opening at i, to its closing fence or the end: one `pre`.
fn code(lines: &[String], i: usize, f: Fence, out: &mut String) -> usize {
    let mut j = i + 1;
    let mut body = vec![];
    while j < lines.len() {
        let t = lines[j].trim();
        let run = t.len() - t.trim_start_matches(f.ch).len();
        if run >= f.len && run == t.len() {
            j += 1;
            break;
        }
        let keep = indent(&lines[j]).min(f.indent);
        body.push(&lines[j][keep..]);
        j += 1;
    }
    out.push_str(&format!("<pre><code>{}</code></pre>", escape(&body.join("\n"))));
    j
}

/// A list item's marker: how far it's indented, its number (an ordered list's), and
/// the column its text starts at.
#[derive(Clone, Copy)]
struct Marker {
    indent: usize,
    number: Option<u64>,
    content: usize,
}

fn marker(line: &str) -> Option<Marker> {
    let indent = indent(line);
    let t = &line[indent..];
    let (number, width) = match t.chars().next()? {
        '-' | '*' | '+' => (None, 1),
        c if c.is_ascii_digit() => {
            let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let after = t[digits..].chars().next();
            if digits > 9 || !matches!(after, Some('.' | ')')) {
                return None;
            }
            (t[..digits].parse().ok(), digits + 1)
        }
        _ => return None,
    };
    let rest = &t[width..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    let gap = if rest.trim().is_empty() || spaces > 4 {
        1
    } else {
        spaces
    };
    Some(Marker {
        indent,
        number,
        content: indent + width + gap,
    })
}

/// A list from its first marker at i: its items, each its lines from its marker's
/// column on (a nested list, a code block), until a line that is neither one of its
/// items nor indented into one.
fn list(lines: &[String], i: usize, first: Marker, out: &mut String) -> usize {
    let ordered = first.number.is_some();
    let mut items: Vec<Vec<String>> = vec![];
    let mut i = i;
    while i < lines.len() {
        let Some(m) = marker(&lines[i]).filter(|m| m.number.is_some() == ordered) else {
            break;
        };
        let (body, next) = item(lines, i, m);
        items.push(body);
        i = next;
        // a blank line between two items keeps the list going
        let after = (i..lines.len()).find(|&j| !blank(&lines[j]));
        match after.and_then(|j| marker(&lines[j]).map(|n| (j, n))) {
            Some((j, n)) if n.number.is_some() == ordered && n.indent < m.content => i = j,
            _ => break,
        }
    }
    let (open, close) = match first.number {
        Some(n) if n != 1 => (format!("<ol start=\"{n}\">"), "</ol>"),
        Some(_) => ("<ol>".to_string(), "</ol>"),
        None => ("<ul>".to_string(), "</ul>"),
    };
    out.push_str(&open);
    for body in items {
        let inner = body.iter().skip_while(|l| blank(l)).collect::<Vec<_>>();
        let loose = inner.windows(2).any(|w| blank(w[0]) && !blank(w[1]));
        out.push_str("<li>");
        blocks(&body, !loose, out);
        out.push_str("</li>");
    }
    out.push_str(close);
    i
}

/// One item's lines, its marker taken off and its own indent taken out, and the line
/// after it.
fn item(lines: &[String], i: usize, m: Marker) -> (Vec<String>, usize) {
    let first = &lines[i];
    let mut body = vec![first.get(m.content..).unwrap_or("").to_string()];
    let mut j = i + 1;
    let mut gap = false;
    while j < lines.len() {
        let l = &lines[j];
        if blank(l) {
            let next = (j..lines.len()).find(|&k| !blank(&lines[k]));
            if !next.is_some_and(|k| indent(&lines[k]) >= m.content) {
                break;
            }
            body.push(String::new());
            gap = true;
        } else if indent(l) >= m.content {
            body.push(l[m.content..].to_string());
            gap = false;
        } else if gap || marker(l).is_some() || fence(l).is_some() || heading(l).is_some() || rule(l) {
            break;
        } else {
            body.push(l.trim_start().to_string()); // a paragraph's line carried on, unindented
        }
        j += 1;
    }
    (body, j)
}

/// Whether a table starts at i: a row with `|` and, under it, the row of dashes.
fn table_at(lines: &[String], i: usize) -> bool {
    let (Some(head), Some(dashes)) = (lines.get(i), lines.get(i + 1)) else {
        return false;
    };
    if !head.contains('|') || !dashes.contains(['|', '-']) {
        return false;
    }
    let delims = cells(dashes);
    let ok = |c: &String| {
        let c = c.trim().trim_start_matches(':').trim_end_matches(':');
        !c.is_empty() && c.chars().all(|x| x == '-')
    };
    !delims.is_empty() && delims.iter().all(ok) && cells(head).len() == delims.len()
}

/// A table row's cells: split at each `|` not escaped, the outer ones dropped.
fn cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") {
        &t[..t.len() - 1]
    } else {
        t
    };
    let mut out = vec![String::new()];
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                out.last_mut().unwrap().push('|');
                chars.next();
            }
            '|' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out.iter().map(|c| c.trim().to_string()).collect()
}

/// A table from its header at i, to the first line that isn't a row.
fn table(lines: &[String], i: usize, out: &mut String) -> usize {
    let head = cells(&lines[i]);
    let n = head.len();
    out.push_str("<table><thead><tr>");
    for c in &head {
        out.push_str(&format!("<th>{}</th>", inline(c)));
    }
    out.push_str("</tr></thead>");
    let mut j = i + 2;
    let mut body = String::new();
    while j < lines.len() && !blank(&lines[j]) && lines[j].contains('|') {
        let mut row = cells(&lines[j]);
        row.resize(n, String::new());
        body.push_str("<tr>");
        for c in &row {
            body.push_str(&format!("<td>{}</td>", inline(c)));
        }
        body.push_str("</tr>");
        j += 1;
    }
    if !body.is_empty() {
        out.push_str(&format!("<tbody>{body}</tbody>"));
    }
    out.push_str("</table>");
    j
}

// ---- inline ----

/// A paragraph's (or a cell's) text: code, emphasis, links; the rest escaped.
fn inline(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    spans(&c, &mut out);
    out
}

fn spans(c: &[char], out: &mut String) {
    let mut i = 0;
    while i < c.len() {
        let next = match c[i] {
            '\\' if c.get(i + 1).is_some_and(|x| x.is_ascii_punctuation()) => {
                out.push_str(&escape(&c[i + 1].to_string()));
                Some(i + 2)
            }
            '`' => code_span(c, i, out),
            '!' if c.get(i + 1) == Some(&'[') => image(c, i, out),
            '[' => link(c, i, out),
            '<' => autolink(c, i, out),
            '*' | '_' => emphasis(c, i, out),
            'h' if i == 0 || !c[i - 1].is_alphanumeric() => bare_url(c, i, out),
            _ => None,
        };
        match next {
            Some(n) => i = n,
            None => {
                out.push_str(&escape(&c[i].to_string()));
                i += 1;
            }
        }
    }
}

/// How many of ch there are in a row from i.
fn run(c: &[char], i: usize, ch: char) -> usize {
    c[i..].iter().take_while(|x| **x == ch).count()
}

/// Where the code span opened by the backticks at i closes: the start of a run of as
/// many backticks.
fn code_close(c: &[char], i: usize) -> Option<usize> {
    let n = run(c, i, '`');
    let mut j = i + n;
    while j < c.len() {
        if c[j] != '`' {
            j += 1;
            continue;
        }
        let m = run(c, j, '`');
        if m == n {
            return Some(j);
        }
        j += m;
    }
    None
}

/// `` `code` ``: closed by a run of as many backticks; a run never closed is text.
fn code_span(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let n = run(c, i, '`');
    let Some(j) = code_close(c, i) else {
        out.push_str(&"`".repeat(n));
        return Some(i + n);
    };
    let inner: String = c[i + n..j].iter().collect();
    let inner = inner.replace('\n', " ");
    let inner = match inner.strip_prefix(' ').and_then(|s| s.strip_suffix(' ')) {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => inner,
    };
    out.push_str(&format!("<code>{}</code>", escape(&inner)));
    Some(j + n)
}

/// `[label](url "title")`: the label's end, and the url and the end of it all.
fn link_parts(c: &[char], i: usize) -> Option<(usize, String, usize)> {
    let mut depth = 0;
    let mut j = i;
    let close = loop {
        match c.get(j)? {
            '\\' => j += 1,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break j;
                }
            }
            _ => {}
        }
        j += 1;
    };
    if c.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut k = close + 2;
    while c.get(k) == Some(&' ') {
        k += 1;
    }
    let mut url = String::new();
    if c.get(k) == Some(&'<') {
        k += 1;
        while *c.get(k)? != '>' {
            url.push(c[k]);
            k += 1;
        }
        k += 1;
    } else {
        let mut parens = 0;
        while let Some(&x) = c.get(k) {
            if x.is_whitespace() || (x == ')' && parens == 0) {
                break;
            }
            parens += (x == '(') as i32 - (x == ')') as i32;
            url.push(x);
            k += 1;
        }
    }
    while c.get(k) == Some(&' ') {
        k += 1;
    }
    if let Some(&q) = c.get(k).filter(|q| matches!(q, '"' | '\'')) {
        k += 1;
        while *c.get(k)? != q {
            k += 1;
        }
        k += 1;
        while c.get(k) == Some(&' ') {
            k += 1;
        }
    }
    (c.get(k) == Some(&')')).then_some((close, url, k + 1))
}

/// A link: to a web page, an `a`; to anything else, its label alone.
fn link(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let (close, url, end) = link_parts(c, i)?;
    let mut label = String::new();
    spans(&c[i + 1..close], &mut label);
    match web(&url) {
        true => out.push_str(&anchor(&url, &label)),
        false => out.push_str(&label),
    }
    Some(end)
}

/// An image: its alt text, as text; nothing is fetched.
fn image(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let (close, _, end) = link_parts(c, i + 1)?;
    let alt: String = c[i + 2..close].iter().collect();
    out.push_str(&escape(&alt));
    Some(end)
}

/// `<https://…>`: a link. Any other `<` is text.
fn autolink(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let close = (i + 1..c.len()).find(|&j| c[j] == '>' || c[j] == '<' || c[j].is_whitespace())?;
    if c[close] != '>' {
        return None;
    }
    let url: String = c[i + 1..close].iter().collect();
    web(&url).then(|| {
        out.push_str(&anchor(&url, &escape(&url)));
        close + 1
    })
}

/// A web address in the text (`https://…`): a link, up to a space, without the
/// punctuation that ends a sentence or a bracket it didn't open.
fn bare_url(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let rest: String = c[i..c.len().min(i + 8)].iter().collect();
    if !rest.starts_with("https://") && !rest.starts_with("http://") {
        return None;
    }
    let mut end = (i..c.len())
        .find(|&j| c[j].is_whitespace() || matches!(c[j], '<' | '>' | '"' | '`'))
        .unwrap_or(c.len());
    loop {
        let last = c[end - 1];
        let opened = c[i..end].iter().filter(|x| **x == '(').count();
        let closed = c[i..end].iter().filter(|x| **x == ')').count();
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_') || (last == ')' && closed > opened) {
            end -= 1;
        } else {
            break;
        }
    }
    let url: String = c[i..end].iter().collect();
    if !url.contains("://") || url.ends_with("://") {
        return None;
    }
    out.push_str(&anchor(&url, &escape(&url)));
    Some(end)
}

/// `*em*`, `**strong**` (or with `_`, never inside a word): the text between rendered
/// again inside. A run never closed is text.
fn emphasis(c: &[char], i: usize, out: &mut String) -> Option<usize> {
    let d = c[i];
    let n = run(c, i, d);
    let word = |x: Option<&char>| x.is_some_and(|x| x.is_alphanumeric());
    let opens = c.get(i + n).is_some_and(|x| !x.is_whitespace()) && !(d == '_' && i > 0 && word(c.get(i - 1)));
    let width = if n >= 2 { 2 } else { 1 };
    let close = opens.then(|| closing(c, i + width, d, width)).flatten();
    match close {
        Some(j) => {
            let (open, shut) = if width == 2 {
                ("<strong>", "</strong>")
            } else {
                ("<em>", "</em>")
            };
            out.push_str(open);
            spans(&c[i + width..j], out);
            out.push_str(shut);
            Some(j + width)
        }
        None => {
            out.push_str(&escape(&d.to_string().repeat(n)));
            Some(i + n)
        }
    }
}

/// Where a run of `width` d's closes the one opened before from: after something, not
/// after a space, and (for `_`) not before a letter. A code span on the way is skipped.
fn closing(c: &[char], from: usize, d: char, width: usize) -> Option<usize> {
    let mut j = from + 1;
    while j < c.len() {
        if c[j] == '`' {
            let n = run(c, j, '`');
            j = code_close(c, j).unwrap_or(j) + n;
            continue;
        }
        if c[j] != d {
            j += 1;
            continue;
        }
        let r = run(c, j, d);
        let after_word = c.get(j + r).is_some_and(|x| x.is_alphanumeric());
        if (r == width || (width == 2 && r == 3)) && !c[j - 1].is_whitespace() && !(d == '_' && after_word) {
            return Some(j + r - width); // `***x***`: the strong closes on the last two
        }
        j += r;
    }
    None
}

/// Whether a link may be one: a web page's address, nothing else (no `javascript:`,
/// `data:`, `file:` or a path on this server).
fn web(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let scheme = lower.starts_with("https://") || lower.starts_with("http://");
    scheme && lower.len() > 8 && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// A link to a web page, opened apart from the page.
fn anchor(url: &str, label: &str) -> String {
    format!(
        "<a href=\"{}\" rel=\"noopener noreferrer\" target=\"_blank\">{label}</a>",
        escape(url)
    )
}

/// The text with every character HTML gives a meaning written as an entity.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/test_markdown.rs"]
mod tests;
