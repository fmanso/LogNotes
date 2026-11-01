//! Reading markdown out of block text.
//!
//! One scanner answers every question the app asks about a block's text: what the link table should
//! hold, how the editor should style it, what a read-only row should render, and which fences a diagram
//! is drawn from. Keeping that in one place is what stops the link table and the styling from disagreeing
//! about what counts as a link or what sits inside code.

/// What a run of block text should look like.
///
/// Flags rather than a single value, so nesting composes: italic inside bold is `BOLD | ITALIC`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Style(u16);

impl Style {
    pub const NONE: Style = Style(0);
    /// A syntax character — `**`, a backtick, `##`, `-`, `>`, the brackets around a link. Stays visible
    /// in an editable block, dimmed, and is dropped entirely from a rendered summary.
    pub const DIM: Style = Style(1 << 0);
    pub const BOLD: Style = Style(1 << 1);
    pub const ITALIC: Style = Style(1 << 2);
    pub const STRIKE: Style = Style(1 << 3);
    /// Inline code, or a line inside a fenced block.
    pub const CODE: Style = Style(1 << 4);
    /// The content of a heading line.
    pub const HEADING: Style = Style(1 << 5);
    /// The visible part of a page link, a tag, or a markdown link's label.
    pub const LINK: Style = Style(1 << 6);

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn contains(self, other: Style) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Style {
    type Output = Style;

    fn bitor(self, other: Style) -> Style {
        Style(self.0 | other.0)
    }
}

/// A styled run of block text, as byte offsets into the block's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

/// What a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[[Page Name]]` — a page in this graph.
    Page,
    /// `#tag` — a page too, written as a tag.
    Tag,
    /// `[label](url)` — somewhere outside the graph.
    Url,
}

/// One link in a block, with the byte range it occupies so a click can be resolved to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    pub target: String,
    pub kind: LinkKind,
    pub start: usize,
    pub end: usize,
}

/// A fenced block: ```lang … ```. A fence with no closing delimiter still counts — that is what a
/// fence looks like while it is being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub lang: String,
    pub code: String,
    /// Where the opening delimiter's line starts, so a caret anywhere from there on is "inside".
    pub open_start: usize,
    /// Where the closing delimiter's line starts, and `None` while the fence is still open.
    pub close_start: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    /// Non-overlapping, in document order.
    pub spans: Vec<Span>,
    /// Every link and tag in the text, in order, one entry per occurrence.
    pub links: Vec<LinkRef>,
    pub fences: Vec<Fence>,
}

/// How deep emphasis may nest before the rest of the text is taken as plain.
const MAX_DEPTH: u8 = 4;

/// Scan one block's content.
pub fn scan(text: &str) -> Scan {
    let mut out = Scan::default();
    let mut open: Option<Fence> = None;
    let mut line_start = 0;

    for line in text.split('\n') {
        let start = line_start;
        let end = start + line.len();
        line_start = end + 1;

        if open.is_some() {
            if line.trim_start().starts_with("```") {
                out.push(start, end, Style::DIM);
                if let Some(mut done) = open.take() {
                    done.close_start = Some(start);
                    out.fences.push(done);
                }
            } else if !line.is_empty() {
                out.push(start, end, Style::CODE);
                if let Some(fence) = open.as_mut() {
                    fence.code.push_str(line);
                    fence.code.push('\n');
                }
            }
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            out.push(start, end, Style::DIM);
            open = Some(Fence {
                lang: trimmed[3..].trim().to_string(),
                code: String::new(),
                open_start: start,
                close_start: None,
            });
            continue;
        }

        scan_line(&mut out, text, start, end);
    }

    if let Some(fence) = open {
        out.fences.push(fence);
    }

    out
}

/// The first ```mermaid fence in the text, if it has one.
pub fn mermaid_fence(text: &str) -> Option<Fence> {
    scan(text)
        .fences
        .into_iter()
        .find(|fence| fence.lang.eq_ignore_ascii_case("mermaid"))
}

/// Whether the caret at `byte_offset` sits inside a fence — on the line that opened it or in its body,
/// but not on the line that closes it. This is what decides whether Enter continues the code block or
/// starts a new block.
pub fn in_fence(text: &str, byte_offset: usize) -> bool {
    scan(text).fences.iter().any(|fence| {
        fence.open_start <= byte_offset && fence.close_start.is_none_or(|end| byte_offset < end)
    })
}

/// Where a caret that a *rendered* block reports belongs in the raw text.
///
/// A block at rest shows its text with the syntax marks removed, so the offset the view hit-tests is
/// counted in the shown text while the editor counts in the raw one. Both are UTF-16 units, as a Qt text
/// document counts them.
pub fn raw_position(text: &str, rendered: usize) -> usize {
    let scan = scan(text);
    let mut raw = 0;
    let mut shown = 0;
    let mut cursor = 0;

    for span in &scan.spans {
        if span.start > cursor {
            let gap = &text[cursor..span.start];
            let length = gap.encode_utf16().count();
            if shown + length > rendered {
                return raw + prefix_utf16(gap, rendered - shown);
            }
            shown += length;
            raw += length;
        }

        let piece = &text[span.start..span.end];
        let length = piece.encode_utf16().count();
        if !span.style.contains(Style::DIM) {
            if shown + length > rendered {
                return raw + prefix_utf16(piece, rendered - shown);
            }
            shown += length;
        }
        raw += length;
        cursor = span.end;
    }

    let tail = &text[cursor..];
    let length = tail.encode_utf16().count();
    if shown + length > rendered {
        return raw + prefix_utf16(tail, rendered - shown);
    }
    raw + length
}

/// How many UTF-16 units the first `units` of `text` take, stopping at a character boundary.
fn prefix_utf16(text: &str, units: usize) -> usize {
    let mut taken = 0;
    for character in text.chars() {
        if taken + character.len_utf16() > units {
            break;
        }
        taken += character.len_utf16();
    }
    taken
}

/// The link or tag covering `byte_offset`, if a click there should navigate.
pub fn link_at(text: &str, byte_offset: usize) -> Option<LinkRef> {
    scan(text)
        .links
        .into_iter()
        .find(|link| link.start <= byte_offset && byte_offset < link.end)
}

/// Render a block as one line of escaped HTML for a read-only row: syntax marks are dropped, the rest
/// is styled, and links carry their target as an href.
///
/// Every character of the note is escaped, so a block that contains markup shows that markup instead of
/// becoming it.
pub fn to_html(text: &str) -> String {
    render(text, false)
}

/// The same rendering for a block that is not being edited: line breaks are kept, so a fence stays a
/// block of code instead of collapsing into one line.
pub fn to_html_block(text: &str) -> String {
    render(text, true)
}

fn render(text: &str, keep_lines: bool) -> String {
    let scan = scan(text);
    let mut html = String::new();
    let mut cursor = 0;

    for span in &scan.spans {
        if span.start > cursor {
            escape_into(&mut html, &text[cursor..span.start], keep_lines);
        }
        if !span.style.contains(Style::DIM) {
            let href = scan
                .links
                .iter()
                .find(|link| link.start <= span.start && span.end <= link.end);
            open_tags(&mut html, span.style, href);
            escape_into(&mut html, &text[span.start..span.end], keep_lines);
            close_tags(&mut html, span.style);
        }
        cursor = cursor.max(span.end);
    }

    if cursor < text.len() {
        escape_into(&mut html, &text[cursor..], keep_lines);
    }

    if keep_lines {
        // A fence's own ``` lines are dim, so dropping them leaves the break that ended them behind:
        // trim those at the edges, but keep blank lines inside the block.
        while let Some(rest) = html.strip_prefix("<br>") {
            html = rest.to_string();
        }
        while let Some(rest) = html.strip_suffix("<br>") {
            html = rest.to_string();
        }
    }

    html
}

fn open_tags(html: &mut String, style: Style, href: Option<&LinkRef>) {
    if style.contains(Style::LINK) {
        html.push_str("<a href=\"");
        match href {
            // A page link and a tag both name a page, so one scheme covers them.
            Some(link) if link.kind != LinkKind::Url => {
                html.push_str("page:");
                escape_into(html, &link.target, false);
            }
            Some(link) => escape_into(html, &link.target, false),
            None => {}
        }
        html.push_str("\">");
    }
    if style.contains(Style::BOLD) || style.contains(Style::HEADING) {
        html.push_str("<b>");
    }
    if style.contains(Style::ITALIC) {
        html.push_str("<i>");
    }
    if style.contains(Style::STRIKE) {
        html.push_str("<s>");
    }
    if style.contains(Style::CODE) {
        html.push_str("<code>");
    }
}

fn close_tags(html: &mut String, style: Style) {
    if style.contains(Style::CODE) {
        html.push_str("</code>");
    }
    if style.contains(Style::STRIKE) {
        html.push_str("</s>");
    }
    if style.contains(Style::ITALIC) {
        html.push_str("</i>");
    }
    if style.contains(Style::BOLD) || style.contains(Style::HEADING) {
        html.push_str("</b>");
    }
    if style.contains(Style::LINK) {
        html.push_str("</a>");
    }
}

/// A newline becomes a space in a summary row, and a line break in a block at rest.
fn escape_into(html: &mut String, text: &str, keep_lines: bool) {
    for character in text.chars() {
        match character {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            '\n' if keep_lines => html.push_str("<br>"),
            '\n' => html.push(' '),
            other => html.push(other),
        }
    }
}

impl Scan {
    fn push(&mut self, start: usize, end: usize, style: Style) {
        if start >= end || style.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last.end == start
            && last.style == style
        {
            last.end = end;
            return;
        }
        self.spans.push(Span { start, end, style });
    }
}

fn scan_line(out: &mut Scan, text: &str, start: usize, end: usize) {
    let line = &text[start..end];
    let (marker, base) = line_prefix(line);
    if marker > 0 {
        out.push(start, start + marker, Style::DIM);
    }
    scan_inline(out, text, start + marker, end, base, 0);
}

/// The line-start syntax a block may open with, and the style its content inherits. The marker length
/// includes the space that ends it, so dropping the marker from a summary drops the gap too.
fn line_prefix(line: &str) -> (usize, Style) {
    let bytes = line.as_bytes();

    let mut hashes = 0;
    while hashes < bytes.len() && hashes < 6 && bytes[hashes] == b'#' {
        hashes += 1;
    }
    if hashes > 0 && (hashes == bytes.len() || bytes[hashes] == b' ') {
        let marker = if hashes < bytes.len() {
            hashes + 1
        } else {
            hashes
        };
        return (marker, Style::HEADING);
    }

    if bytes.first() == Some(&b'>') && (bytes.len() == 1 || bytes[1] == b' ') {
        return (if bytes.len() == 1 { 1 } else { 2 }, Style::ITALIC);
    }

    if let Some(first) = bytes.first()
        && matches!(first, b'-' | b'*' | b'+')
        && bytes.get(1) == Some(&b' ')
    {
        return (2, Style::NONE);
    }

    let mut digits = 0;
    while digits < bytes.len() && digits < 9 && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0
        && matches!(bytes.get(digits), Some(b'.') | Some(b')'))
        && bytes.get(digits + 1) == Some(&b' ')
    {
        return (digits + 2, Style::NONE);
    }

    (0, Style::NONE)
}

fn scan_inline(out: &mut Scan, text: &str, from: usize, to: usize, base: Style, depth: u8) {
    let mut i = from;
    let mut plain = from;

    while i < to {
        let rest = &text[i..to];
        let Some(character) = rest.chars().next() else {
            break;
        };
        let mut handled = false;

        if character == '`' {
            if let Some(close) = find(text, i + 1, to, "`") {
                push_plain(out, plain, i, base);
                out.push(i, i + 1, Style::DIM);
                out.push(i + 1, close, base | Style::CODE);
                out.push(close, close + 1, Style::DIM);
                i = close + 1;
                plain = i;
                handled = true;
            }
        } else if character == '[' {
            if rest.starts_with("[[") {
                if let Some(close) = find(text, i + 2, to, "]]") {
                    push_plain(out, plain, i, base);
                    out.push(i, i + 2, Style::DIM);
                    out.push(i + 2, close, base | Style::LINK);
                    out.push(close, close + 2, Style::DIM);
                    let target = text[i + 2..close].trim();
                    if !target.is_empty() {
                        out.links.push(LinkRef {
                            target: target.to_string(),
                            kind: LinkKind::Page,
                            start: i,
                            end: close + 2,
                        });
                    }
                    i = close + 2;
                    plain = i;
                    handled = true;
                }
            } else if let Some((label_end, link_end)) = markdown_link(text, i, to) {
                push_plain(out, plain, i, base);
                out.push(i, i + 1, Style::DIM);
                out.push(i + 1, label_end, base | Style::LINK);
                out.push(label_end, link_end, Style::DIM);
                let target = text[label_end + 2..link_end - 1].trim();
                if !target.is_empty() {
                    out.links.push(LinkRef {
                        target: target.to_string(),
                        kind: LinkKind::Url,
                        start: i,
                        end: link_end,
                    });
                }
                i = link_end;
                plain = i;
                handled = true;
            }
        } else if depth < MAX_DEPTH {
            // Each of these flushes the plain run first, then hands back the position after the
            // closing marker so the loop can carry on past it.
            let emphasis = if rest.starts_with("~~") {
                Some(("~~", Style::STRIKE))
            } else if rest.starts_with("**") {
                Some(("**", Style::BOLD))
            } else if (character == '*' || character == '_')
                && emphasis_starts(text, i, marker_of(character))
            {
                Some((marker_of(character), Style::ITALIC))
            } else {
                None
            };

            if let Some((marker, style)) = emphasis {
                push_plain(out, plain, i, base);
                if let Some(end) = emphasis_end(out, text, i, to, marker, style, base, depth) {
                    i = end;
                    plain = i;
                    handled = true;
                }
            }
        }

        if !handled && character == '#' {
            if let Some(token_end) = tag_end(text, i + 1, to) {
                push_plain(out, plain, i, base);
                out.push(i, token_end, base | Style::LINK);
                out.links.push(LinkRef {
                    target: text[i + 1..token_end].to_string(),
                    kind: LinkKind::Tag,
                    start: i,
                    end: token_end,
                });
                // Anything the tag stopped at — a comma, a bracket — stays as plain text.
                i = token_end;
                plain = i;
                handled = true;
            }
        }

        if !handled {
            i += character.len_utf8();
        }
    }

    push_plain(out, plain, to, base);
}

/// Style the run between a pair of markers, recursing so `**bold *and italic***` composes. Returns the
/// position just past the closing marker, or `None` when the pair is not closed.
fn emphasis_end(
    out: &mut Scan,
    text: &str,
    i: usize,
    to: usize,
    marker: &str,
    style: Style,
    base: Style,
    depth: u8,
) -> Option<usize> {
    let close = find(text, i + marker.len(), to, marker)?;
    if close == i + marker.len() {
        return None;
    }
    out.push(i, i + marker.len(), Style::DIM);
    scan_inline(out, text, i + marker.len(), close, base | style, depth + 1);
    out.push(close, close + marker.len(), Style::DIM);
    Some(close + marker.len())
}

fn marker_of(character: char) -> &'static str {
    if character == '*' { "*" } else { "_" }
}

/// A `*` or `_` opens emphasis only when it is followed by something that is not a space, and `_` only
/// when it does not sit inside a word — otherwise `snake_case_names` would italicise.
fn emphasis_starts(text: &str, i: usize, marker: &str) -> bool {
    let after = text[i + marker.len()..].chars().next();
    let before = text[..i].chars().next_back();
    let after_ok = after.map(|c| !c.is_whitespace()).unwrap_or(false);
    let before_ok = marker == "*" || before.map(|c| !c.is_alphanumeric()).unwrap_or(true);
    after_ok && before_ok
}

/// A tag: `#` and one token with no whitespace, stopping at punctuation. Mirrors what the link table
/// accepts, so a tag is never styled without being recorded.
fn tag_end(text: &str, from: usize, to: usize) -> Option<usize> {
    let mut end = from;
    while end < to {
        let Some(character) = text[end..to].chars().next() else {
            break;
        };
        if character.is_whitespace() || is_tag_break(character) {
            break;
        }
        end += character.len_utf8();
    }
    let raw = &text[from..end];
    let trimmed = raw.trim_end_matches(['.', ',', ';', ':']);
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    Some(from + trimmed.len())
}

fn is_tag_break(character: char) -> bool {
    matches!(character, '#' | '[' | ']' | '(' | ')' | '`')
}

/// `[label](url)` — returns the byte index of `]` and the index just past `)`.
fn markdown_link(text: &str, i: usize, to: usize) -> Option<(usize, usize)> {
    let label_end = find(text, i + 1, to, "]")?;
    if text.as_bytes().get(label_end + 1) != Some(&b'(') {
        return None;
    }
    let close = find(text, label_end + 2, to, ")")?;
    Some((label_end, close + 1))
}

fn push_plain(out: &mut Scan, start: usize, end: usize, style: Style) {
    if style.is_empty() || start >= end {
        return;
    }
    out.push(start, end, style);
}

fn find(text: &str, from: usize, to: usize, needle: &str) -> Option<usize> {
    if from >= to {
        return None;
    }
    text[from..to].find(needle).map(|at| from + at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(text: &str) -> Vec<(String, Style)> {
        scan(text)
            .spans
            .into_iter()
            .map(|span| (text[span.start..span.end].to_string(), span.style))
            .collect()
    }

    fn targets(text: &str) -> Vec<String> {
        scan(text)
            .links
            .into_iter()
            .map(|link| link.target)
            .collect()
    }

    #[test]
    fn bold_keeps_its_marks_and_styles_the_words() {
        let found = styles("call the **plumber** now");

        assert_eq!(
            found,
            [
                ("**".to_string(), Style::DIM),
                ("plumber".to_string(), Style::BOLD),
                ("**".to_string(), Style::DIM),
            ]
        );
    }

    #[test]
    fn italic_strike_and_code_each_mark_their_own_content() {
        assert_eq!(
            styles("*soon*"),
            [
                ("*".to_string(), Style::DIM),
                ("soon".to_string(), Style::ITALIC),
                ("*".to_string(), Style::DIM),
            ]
        );
        assert_eq!(
            styles("~~gone~~"),
            [
                ("~~".to_string(), Style::DIM),
                ("gone".to_string(), Style::STRIKE),
                ("~~".to_string(), Style::DIM),
            ]
        );
        assert_eq!(
            styles("`noon`"),
            [
                ("`".to_string(), Style::DIM),
                ("noon".to_string(), Style::CODE),
                ("`".to_string(), Style::DIM),
            ]
        );
    }

    #[test]
    fn emphasis_nests() {
        let found = styles("**bold *and italic* here**");

        assert!(found.contains(&("and italic".to_string(), Style::BOLD | Style::ITALIC)));
        assert!(found.contains(&("bold ".to_string(), Style::BOLD)));
        assert!(found.contains(&(" here".to_string(), Style::BOLD)));
    }

    #[test]
    fn an_unclosed_marker_is_just_text() {
        assert!(styles("2 * 3 and **half").is_empty());
        assert!(styles("a `b").is_empty());
    }

    #[test]
    fn underscores_inside_words_are_not_emphasis() {
        assert!(styles("snake_case_name").is_empty());
        assert_eq!(
            styles("_slanted_").first().map(|(_, style)| *style),
            Some(Style::DIM)
        );
    }

    #[test]
    fn links_and_tags_are_found_with_their_ranges() {
        let scan = scan("see [[My Project]] and #idea");

        assert_eq!(
            scan.links
                .iter()
                .map(|link| (link.target.as_str(), link.kind))
                .collect::<Vec<_>>(),
            [("My Project", LinkKind::Page), ("idea", LinkKind::Tag)]
        );
        let page = &scan.links[0];
        assert_eq!(
            &"see [[My Project]] and #idea"[page.start..page.end],
            "[[My Project]]"
        );
    }

    #[test]
    fn a_tag_stops_at_punctuation_and_leaves_it_plain() {
        let scan = scan("#idea, then");

        assert_eq!(targets("#idea, then"), ["idea"]);
        assert_eq!(scan.links[0].end, "#idea".len());
    }

    #[test]
    fn a_markdown_link_is_styled_and_points_outward() {
        let scan = scan("read [the docs](https://example.com/x) first");

        assert_eq!(scan.links[0].kind, LinkKind::Url);
        assert_eq!(scan.links[0].target, "https://example.com/x");
        assert!(
            styles("read [the docs](https://example.com/x) first")
                .contains(&("the docs".to_string(), Style::LINK))
        );
    }

    #[test]
    fn a_code_span_hides_its_links() {
        assert!(targets("a `[[Not A Link]]` b").is_empty());
        assert_eq!(targets("`x` [[Real]]"), ["Real"]);
    }

    #[test]
    fn a_fence_collects_its_lines_and_hides_their_links() {
        let scan = scan("```mermaid\ngraph TD\n  A --> [[B]]\n```");

        assert_eq!(scan.fences.len(), 1);
        assert_eq!(scan.fences[0].lang, "mermaid");
        assert_eq!(scan.fences[0].code, "graph TD\n  A --> [[B]]\n");
        assert!(scan.links.is_empty(), "a fence's contents are code");
        assert!(
            styles("```mermaid\ngraph TD\n```")
                .iter()
                .any(|(text, style)| { text == "graph TD" && *style == Style::CODE })
        );
    }

    #[test]
    fn a_fence_that_is_still_being_typed_still_counts() {
        let scan = scan("```mermaid\ngraph TD\n  A --> B");

        assert_eq!(scan.fences.len(), 1);
        assert_eq!(scan.fences[0].code, "graph TD\n  A --> B\n");
    }

    #[test]
    fn mermaid_fences_are_found_by_language() {
        assert!(mermaid_fence("```rust\nfn main() {}\n```").is_none());
        assert!(mermaid_fence("```Mermaid\ngraph TD\n```").is_some());
    }

    #[test]
    fn a_heading_styles_its_content_and_dims_its_hashes() {
        assert_eq!(
            styles("## Plan"),
            [
                ("## ".to_string(), Style::DIM),
                ("Plan".to_string(), Style::HEADING),
            ]
        );
    }

    #[test]
    fn a_list_marker_and_a_quote_marker_are_dimmed() {
        assert_eq!(styles("- milk")[0], ("- ".to_string(), Style::DIM));
        assert_eq!(styles("1. milk")[0], ("1. ".to_string(), Style::DIM));
        assert_eq!(
            styles("> remember"),
            [
                ("> ".to_string(), Style::DIM),
                ("remember".to_string(), Style::ITALIC),
            ]
        );
    }

    #[test]
    fn a_line_that_only_looks_like_a_marker_is_not_one() {
        // `#hashtag` is a tag, not a heading; `-milk` is just text.
        assert!(
            styles("#hashtag")
                .iter()
                .all(|(_, style)| *style != Style::HEADING)
        );
        assert!(styles("-milk").is_empty());
    }

    #[test]
    fn spans_stay_inside_character_boundaries() {
        let text = "héllo 🎉 **wörld** `x`";
        let scan = scan(text);

        for span in &scan.spans {
            assert!(
                text.is_char_boundary(span.start),
                "start {} splits a character",
                span.start
            );
            assert!(
                text.is_char_boundary(span.end),
                "end {} splits a character",
                span.end
            );
        }
        assert!(styles(text).contains(&("wörld".to_string(), Style::BOLD)));
    }

    #[test]
    fn a_click_resolves_to_the_link_under_it() {
        let text = "see [[My Project]] now";

        let hit = link_at(text, text.find("My").unwrap()).expect("inside the link");
        assert_eq!(hit.target, "My Project");
        assert!(link_at(text, 1).is_none(), "plain text is not a link");
    }

    #[test]
    fn html_drops_the_marks_and_keeps_the_styling() {
        assert_eq!(
            to_html("call the **plumber** before `noon`"),
            "call the <b>plumber</b> before <code>noon</code>"
        );
        assert_eq!(to_html("## Plan"), "<b>Plan</b>");
        assert_eq!(to_html("- milk"), "milk");
        assert_eq!(to_html("> remember"), "<i>remember</i>");
    }

    #[test]
    fn html_links_pages_tags_and_urls() {
        assert_eq!(
            to_html("see [[My Project]] and #idea"),
            // A tag keeps its `#`: that is how it reads, and dropping it would make the summary lie.
            "see <a href=\"page:My Project\">My Project</a> and <a href=\"page:idea\">#idea</a>"
        );
        assert_eq!(
            to_html("[the docs](https://example.com/x)"),
            "<a href=\"https://example.com/x\">the docs</a>"
        );
    }

    #[test]
    fn html_escapes_everything_the_note_says() {
        assert_eq!(
            to_html("<img src=x onerror=\"alert('hi')\">"),
            "&lt;img src=x onerror=&quot;alert(&#39;hi&#39;)&quot;&gt;"
        );
        assert_eq!(
            to_html("[[a\"b]]"),
            "<a href=\"page:a&quot;b\">a&quot;b</a>"
        );
    }

    #[test]
    fn a_fence_keeps_the_caret_inside_until_it_closes() {
        let text = "```mermaid\ngraph TD\n  A --> B\n```";

        assert!(in_fence(text, 0), "the line that opens it is inside");
        assert!(
            in_fence(text, text.find("graph").unwrap()),
            "so is its body"
        );
        assert!(in_fence(text, text.find("A -->").unwrap()));
        assert!(
            !in_fence(text, text.rfind("```").unwrap()),
            "the closing line is not"
        );
        assert!(!in_fence("just a note", 3));
    }

    #[test]
    fn a_fence_that_was_never_closed_stays_open() {
        let text = "```mermaid\ngraph TD";

        assert!(in_fence(text, text.len()), "so Enter keeps adding lines");
    }

    #[test]
    fn a_rendered_caret_maps_back_past_the_marks() {
        let text = "**bold** and `code`";

        // The shown text is "bold and code": `and` starts at 5 there and at 9 in the raw text.
        assert_eq!(raw_position(text, 5), 9);
        assert_eq!(
            raw_position(text, 9),
            14,
            "the c of code, past its backtick"
        );
        assert_eq!(raw_position(text, 10), 15);
        // A click on the first shown character lands before it — inside the marks, where that
        // character starts.
        assert_eq!(raw_position(text, 0), 2);
        assert_eq!(
            raw_position(text, usize::MAX / 2),
            text.len(),
            "past the end"
        );
    }

    #[test]
    fn the_caret_mapping_counts_utf16_units() {
        let text = "🎉 **bold**";

        // The emoji is two UTF-16 units, so `b` is at 3 in the shown text and at 5 in the raw one.
        assert_eq!(raw_position(text, 3), 5);
    }

    #[test]
    fn a_block_at_rest_keeps_its_line_breaks() {
        let html = to_html_block("```mermaid\ngraph TD\n  A --> B\n```");

        assert_eq!(html, "<code>graph TD</code><br><code>  A --&gt; B</code>");
        assert_eq!(to_html_block("first\nsecond"), "first<br>second");
        assert_eq!(
            to_html("first\nsecond"),
            "first second",
            "a summary stays one line"
        );
    }

    #[test]
    fn html_keeps_a_multi_line_block_on_one_line() {
        assert_eq!(to_html("first\nsecond"), "first second");
    }
}
