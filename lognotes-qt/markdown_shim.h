// Styling a block's text as markdown: the editor draws formatting, the stored text stays markdown.
//
// The scanning lives in Rust — `lognotes_core::markdown` — and so does the palette. This file only turns
// the spans it is handed into QTextCharFormats, which is the one thing a Qt text document cannot be told
// from Rust. Keeping the split there is what stops the editor's styling from disagreeing with the link
// table about what is a link or what sits inside code.

#pragma once

#include <QSyntaxHighlighter>
#include <QString>
#include <QtGlobal>

#include <cstddef>
#include <cstdint>

class QVariant;

// One styled run, already resolved by Rust: it carries the final weight, size and colours, so nothing
// here needs to know what markdown is. Field order and size are pinned on both sides — see the
// static_assert below and the size test in src/markdown.rs.
struct LogNotesFormat {
    quint32 start;      // UTF-16 offset within the block
    quint32 length;     // UTF-16 length
    quint32 weight;     // 0 = leave alone, else a QFont::Weight (400, 700)
    float pointSize;    // 0 = leave alone
    quint32 foreground; // 0xRRGGBB, 0 = leave alone
    quint32 background; // 0xRRGGBB, 0 = leave alone
    quint32 italic;
    quint32 strike;
    quint32 underline;
    quint32 monospace;
};

static_assert(sizeof(LogNotesFormat) == 40, "LogNotesFormat must match the Rust struct byte for byte");

// Implemented in Rust (src/markdown.rs). Styles `blockLength` UTF-16 units starting at `blockStart`, as
// offsets relative to that start, and returns how many runs the block has — so a caller that
// under-sized its buffer can ask again.
extern "C" std::size_t lognotes_markdown_formats(const char *utf8, std::size_t length,
                                                 std::uint32_t blockStart,
                                                 std::uint32_t blockLength,
                                                 LogNotesFormat *out, std::size_t capacity);

// Styles `document`'s text as markdown for the rest of its life. False when it is not a text document.
bool lognotes_install_highlighter(const QVariant& document);
