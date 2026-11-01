#include "markdown_shim.h"

#include <QByteArray>
#include <QDebug>
#include <QColor>
#include <QFontDatabase>
#include <QQuickTextDocument>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QVariant>

#include <vector>

namespace {

// Formats the block it is asked about, one block at a time, from spans Rust computed for that block.
//
// Only character formats are set: the document's characters are never touched, so the caret, the undo
// stack and the markdown that gets saved all behave exactly as they did before the text was styled.
class MarkdownHighlighter : public QSyntaxHighlighter {
public:
    using QSyntaxHighlighter::QSyntaxHighlighter;

protected:
    void highlightBlock(const QString &text) override
    {
        const QTextBlock block = currentBlock();
        if (!block.isValid() || text.isEmpty()) {
            return;
        }

        // The whole text, not just this paragraph: markdown is not line-oriented, and a fence opened on
        // an earlier line is what decides that this one is code.
        const QByteArray utf8 = block.document()->toPlainText().toUtf8();
        const auto start = static_cast<std::uint32_t>(block.position());
        const auto length = static_cast<std::uint32_t>(text.size());

        constexpr std::size_t onStack = 128;
        LogNotesFormat local[onStack];

        std::size_t count = lognotes_markdown_formats(
            utf8.constData(), std::size_t(utf8.size()), start, length, local, onStack);

        // A paragraph-long block is rare, but a fence with a whole diagram in it can exceed the stack
        // buffer; ask again with room to spare rather than dropping the tail of the styling.
        std::vector<LogNotesFormat> spare;
        const LogNotesFormat *formats = local;
        if (count > onStack) {
            spare.resize(count);
            count = lognotes_markdown_formats(
                utf8.constData(), std::size_t(utf8.size()), start, length, spare.data(), spare.size());
            formats = spare.data();
        }

        for (std::size_t i = 0; i < count; ++i) {
            const LogNotesFormat &entry = formats[i];
            QTextCharFormat format;
            if (entry.weight != 0) {
                format.setFontWeight(static_cast<int>(entry.weight));
            }
            if (entry.pointSize > 0) {
                format.setFontPointSize(entry.pointSize);
            }
            if (entry.foreground != 0) {
                format.setForeground(QColor::fromRgb(entry.foreground));
            }
            if (entry.background != 0) {
                format.setBackground(QColor::fromRgb(entry.background));
            }
            if (entry.italic) {
                format.setFontItalic(true);
            }
            if (entry.strike) {
                format.setFontStrikeOut(true);
            }
            if (entry.underline) {
                format.setFontUnderline(true);
            }
            if (entry.monospace) {
                format.setFontFamilies(
                    {QFontDatabase::systemFont(QFontDatabase::FixedFont).family()});
            }
            setFormat(static_cast<int>(entry.start), static_cast<int>(entry.length), format);
        }
    }
};

} // namespace

bool lognotes_install_highlighter(const QVariant& document)
{
    // QML hands over the TextEdit's `textDocument` as a variant; unwrap it to the QQuickTextDocument,
    // and from there to the QTextDocument the highlighter actually styles.
    const auto *wrapper = qobject_cast<const QQuickTextDocument*>(document.value<QObject*>());
    QTextDocument *text = wrapper ? wrapper->textDocument() : nullptr;
    if (!text) {
        return false;
    }

    auto *highlighter = new MarkdownHighlighter(text);
    // Owned by the document it styles: a delegate rebuild makes a new document and a new highlighter,
    // and the old pair goes away with the old delegate.
    highlighter->setParent(text);
    highlighter->rehighlight();
    return true;
}
