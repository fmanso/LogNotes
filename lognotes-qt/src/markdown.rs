//! Resolving a block's markdown into the styling the editor draws.
//!
//! The scan lives in `lognotes_core::markdown` — one scanner for the link table, the editor and the
//! rendered summaries. This module only decides what each style *looks like*, so the palette has a
//! single home and `markdown_shim.cpp` stays ignorant of markdown: it applies numbers, nothing more.

use lognotes_core::markdown::{Style, scan};

/// A styled run in the shape the C++ highlighter reads. The layout is part of the contract with
/// `markdown_shim.h` (`static_assert(sizeof(LogNotesFormat) == 40)` there, the test below here).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Format {
    /// UTF-16 offset within the block, which is how a QTextDocument addresses its characters.
    pub start: u32,
    pub length: u32,
    /// 0 leaves the weight alone.
    pub weight: u32,
    /// 0 leaves the size alone.
    pub point_size: f32,
    /// `0xRRGGBB`, or 0 to leave it alone.
    pub foreground: u32,
    /// `0xRRGGBB`, or 0 to leave it alone.
    pub background: u32,
    pub italic: u32,
    pub strike: u32,
    pub underline: u32,
    pub monospace: u32,
}

/// Body text is 16 px in the column; a heading is a step larger and bold.
const HEADING_POINT_SIZE: f32 = 19.0;
const BOLD_WEIGHT: u32 = 700;
/// The muted grey the column already uses for chips and section headings.
const MARK_COLOUR: u32 = 0x8a_85_7c;
/// The blue the column already uses for `DOING`.
const LINK_COLOUR: u32 = 0x3a_6e_a5;
/// A tint a shade off the page's cream, so code reads as a run rather than as emphasis.
const CODE_BACKGROUND: u32 = 0xf0_ec_e4;

/// The runs to style in `text`, in document order, with byte offsets resolved to UTF-16.
pub fn formats(text: &str) -> Vec<Format> {
    let spans = scan(text).spans;
    let mut formats = Vec::with_capacity(spans.len());
    let mut byte = 0;
    let mut utf16 = 0u32;

    for span in spans {
        while byte < span.start {
            let character = text[byte..].chars().next().expect("span splits a character");
            byte += character.len_utf8();
            utf16 += character.len_utf16() as u32;
        }
        let start = utf16;
        while byte < span.end {
            let character = text[byte..].chars().next().expect("span splits a character");
            byte += character.len_utf8();
            utf16 += character.len_utf16() as u32;
        }

        let mut format = Format {
            start,
            length: utf16 - start,
            ..Format::default()
        };
        resolve(&mut format, span.style);
        formats.push(format);
    }

    formats
}

/// The runs to style in one paragraph of `text` — the whole block text, with the paragraph singled out
/// by `block_start` and `block_length` in UTF-16 units, rebased to start at 0.
///
/// A highlighter is handed one paragraph at a time, but markdown is not line-oriented: a ``` fence
/// opens on one line and closes several lines later, so the scan has to see all of the text and only
/// the *answer* is cut down to the paragraph.
pub fn block_formats(text: &str, block_start: u32, block_length: u32) -> Vec<Format> {
    let block_end = block_start.saturating_add(block_length);

    formats(text)
        .into_iter()
        .filter_map(|format| {
            let start = format.start.max(block_start);
            let end = format.start.saturating_add(format.length).min(block_end);
            if end <= start {
                return None;
            }
            Some(Format {
                start: start - block_start,
                length: end - start,
                ..format
            })
        })
        .collect()
}

fn resolve(format: &mut Format, style: Style) {
    if style.contains(Style::DIM) {
        // The syntax characters stay on screen — they are what the note is — but step back.
        format.foreground = MARK_COLOUR;
    }
    if style.contains(Style::BOLD) || style.contains(Style::HEADING) {
        format.weight = BOLD_WEIGHT;
    }
    if style.contains(Style::HEADING) {
        format.point_size = HEADING_POINT_SIZE;
    }
    if style.contains(Style::ITALIC) {
        format.italic = 1;
    }
    if style.contains(Style::STRIKE) {
        format.strike = 1;
    }
    if style.contains(Style::CODE) {
        format.monospace = 1;
        format.background = CODE_BACKGROUND;
    }
    if style.contains(Style::LINK) {
        format.foreground = LINK_COLOUR;
        format.underline = 1;
    }
}

/// Called by `markdown_shim.cpp` once per paragraph, on the GUI thread.
///
/// # Safety
///
/// `text` must point at `length` readable bytes of UTF-8, and `out` at `capacity` writable `Format`s
/// (or be null, which just asks how many there are).
#[unsafe(no_mangle)]
pub extern "C" fn lognotes_markdown_formats(
    text: *const u8,
    length: usize,
    block_start: u32,
    block_length: u32,
    out: *mut Format,
    capacity: usize,
) -> usize {
    if text.is_null() {
        return 0;
    }
    // SAFETY: the caller is the highlighter, which passes a live QByteArray's buffer and its size.
    let bytes = unsafe { std::slice::from_raw_parts(text, length) };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 0;
    };

    let formats = block_formats(text, block_start, block_length);
    if out.is_null() {
        return formats.len();
    }
    let written = formats.len().min(capacity);
    // SAFETY: `out` has room for `capacity` elements, and `written` never exceeds it.
    unsafe { std::ptr::copy_nonoverlapping(formats.as_ptr(), out, written) };
    formats.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn the_format_layout_matches_the_cpp_struct() {
        // markdown_shim.h asserts the same size from the other side.
        assert_eq!(size_of::<Format>(), 40);
    }

    #[test]
    fn a_bold_run_carries_the_words_and_dims_its_marks() {
        let text = "**bold**";
        let formats = formats(text);

        assert_eq!(formats.len(), 3);
        assert_eq!((formats[0].start, formats[0].length), (0, 2));
        assert_eq!(formats[0].foreground, MARK_COLOUR);
        assert_eq!((formats[1].start, formats[1].length), (2, 4));
        assert_eq!(formats[1].weight, BOLD_WEIGHT);
        assert_eq!(formats[1].foreground, 0, "the words keep the editor's colour");
    }

    #[test]
    fn offsets_are_utf16_not_bytes() {
        // The emoji is four bytes of UTF-8 and two of UTF-16, so a byte offset would land mid-word.
        let text = "🎉 **bold**";
        let formats = formats(text);
        let bold = formats.iter().find(|f| f.weight == BOLD_WEIGHT).expect("bold run");

        let expected: u32 = text[..text.find("bold").unwrap()].encode_utf16().count() as u32;
        assert_eq!(bold.start, expected);
    }

    #[test]
    fn a_link_is_blue_and_underlined_a_heading_is_bigger() {
        let link = formats("[[Page]]")
            .into_iter()
            .find(|f| f.underline == 1)
            .expect("a link run");
        assert_eq!(link.foreground, LINK_COLOUR);

        let heading = formats("## Plan")
            .into_iter()
            .find(|f| f.point_size > 0.0)
            .expect("a heading run");
        assert_eq!(heading.weight, BOLD_WEIGHT);
        assert!(heading.point_size > 16.0, "the body text is 16 px");
    }

    #[test]
    fn code_gets_a_background_and_a_fixed_pitch_face() {
        let code = formats("`x`")
            .into_iter()
            .find(|f| f.monospace == 1)
            .expect("a code run");
        assert_eq!(code.background, CODE_BACKGROUND);
    }

    #[test]
    fn a_paragraph_is_cut_out_of_the_whole_text() {
        let text = "one\n```mermaid\ngraph TD\n```";
        let blocks: Vec<&str> = text.split('\n').collect();
        let mut start = 0u32;
        let mut spans_for = Vec::new();
        for block in &blocks {
            let length = block.encode_utf16().count() as u32;
            spans_for.push(block_formats(text, start, length));
            start += length + 1;
        }

        let fence_body = &spans_for[2];
        assert_eq!(fence_body.len(), 1, "the diagram line is styled");
        assert_eq!(fence_body[0].start, 0, "and rebased to the paragraph");
        assert_eq!(
            fence_body[0].length,
            "graph TD".encode_utf16().count() as u32
        );
        assert_eq!(fence_body[0].background, CODE_BACKGROUND, "as code");
    }

    #[test]
    fn the_ffi_reports_the_count_and_stops_at_capacity() {
        let text = "**bold**";
        let bytes = text.as_bytes();
        let needed = lognotes_markdown_formats(
            bytes.as_ptr(),
            bytes.len(),
            0,
            text.encode_utf16().count() as u32,
            std::ptr::null_mut(),
            0,
        );
        assert_eq!(needed, formats(text).len());

        let mut small = [Format::default(); 1];
        let written = lognotes_markdown_formats(
            bytes.as_ptr(),
            bytes.len(),
            0,
            text.encode_utf16().count() as u32,
            small.as_mut_ptr(),
            1,
        );
        assert_eq!(written, needed, "the caller is told the real count");
        assert_eq!(small[0].start, 0, "and gets the first run in its buffer");
    }
}
