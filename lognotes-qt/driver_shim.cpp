#include <lognotes-qt/driver_shim.h>

#include <QAbstractItemModel>
#include <QDebug>
#include <QCoreApplication>
#include <QList>
#include <QEventLoop>
#include <QImage>
#include <QKeyEvent>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QQuickTextDocument>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QTextLayout>
#include <QTimer>
#include <QWheelEvent>
#include <QPointingDevice>
#include <QMouseEvent>

namespace {
QObject* g_root = nullptr;

QQuickWindow* window() {
    return qobject_cast<QQuickWindow*>(g_root);
}

// The Journal instance is a child of the root window in QML.
QAbstractItemModel* journal_model() {
    if (!g_root) {
        return nullptr;
    }
    const auto models = g_root->findChildren<QAbstractItemModel*>();
    return models.isEmpty() ? nullptr : models.first();
}
} // namespace

bool lognotes_attach(const QQmlApplicationEngine& engine) {
    if (engine.rootObjects().isEmpty()) {
        return false;
    }
    g_root = engine.rootObjects().first();
    return g_root != nullptr;
}

bool lognotes_type_text(const QString& text) {
    auto* w = window();
    if (!w) {
        return false;
    }
    // One character at a time, as a keyboard would: Qt routes these to the focus item.
    for (const QChar& character : text) {
        const QString keys(character);
        QKeyEvent press(QEvent::KeyPress, 0, Qt::NoModifier, keys);
        QCoreApplication::sendEvent(w, &press);
        QKeyEvent release(QEvent::KeyRelease, 0, Qt::NoModifier, keys);
        QCoreApplication::sendEvent(w, &release);
    }
    return true;
}

bool lognotes_press_key(int key, int modifiers) {
    auto* w = window();
    if (!w) {
        return false;
    }
    const auto mods = static_cast<Qt::KeyboardModifiers>(modifiers);
    QKeyEvent press(QEvent::KeyPress, key, mods);
    QCoreApplication::sendEvent(w, &press);
    QKeyEvent release(QEvent::KeyRelease, key, mods);
    QCoreApplication::sendEvent(w, &release);
    return true;
}

bool lognotes_save_png(const QString& path) {
    auto* w = window();
    if (!w) {
        return false;
    }
    return w->grabWindow().save(path);
}

void lognotes_wait_ms(int ms) {
    QEventLoop loop;
    QTimer::singleShot(ms, &loop, &QEventLoop::quit);
    loop.exec();
}

QString lognotes_focus_name() {
    auto* w = window();
    if (!w) {
        return {};
    }
    QQuickItem* item = w->activeFocusItem();
    if (!item) {
        return QStringLiteral("<none>");
    }
    return item->objectName().isEmpty() ? QString::fromLatin1(item->metaObject()->className())
                                        : item->objectName();
}

namespace {
void collect_editors(QQuickItem* item, QList<QQuickItem*>& out) {
    if (!item) {
        return;
    }
    if (item->objectName() == QLatin1String("blockEditor") && item->isVisible()) {
        out.append(item);
    }
    // Walk the visual tree: ListView delegates live under the content item, not as QObject
    // children of the window.
    for (auto* child : item->childItems()) {
        collect_editors(child, out);
    }
}

QList<QQuickItem*> block_editors() {
    QList<QQuickItem*> editors;
    if (auto* w = window()) {
        collect_editors(w->contentItem(), editors);
    }
    return editors;
}
} // namespace

bool lognotes_click_block(int index) {
    auto* w = window();
    const auto editors = block_editors();
    if (!w || index < 0 || index >= editors.size()) {
        return false;
    }
    auto* item = editors.at(index);
    const QPointF centre = item->mapToScene(QPointF(item->width() / 2.0, item->height() / 2.0));
    const QPointF global = w->mapToGlobal(centre.toPoint());

    QMouseEvent press(QEvent::MouseButtonPress, centre, global, Qt::LeftButton, Qt::LeftButton,
                      Qt::NoModifier);
    QCoreApplication::sendEvent(w, &press);
    QMouseEvent release(QEvent::MouseButtonRelease, centre, global, Qt::LeftButton, Qt::NoButton,
                        Qt::NoModifier);
    QCoreApplication::sendEvent(w, &release);
    return true;
}

QString lognotes_editor_text(int index) {
    const auto editors = block_editors();
    if (index < 0 || index >= editors.size()) {
        return {};
    }
    return editors.at(index)->property("text").toString();
}

QString lognotes_editor_debug(int index) {
    const auto editors = block_editors();
    if (index < 0 || index >= editors.size()) {
        return QStringLiteral("<none>");
    }
    auto* item = editors.at(index);
    const QPointF scene = item->mapToScene(QPointF(0, 0));
    return QStringLiteral("%1 w=%2 h=%3 at(%4,%5) visible=%6")
        .arg(item->property("text").toString())
        .arg(item->width())
        .arg(item->height())
        .arg(scene.x())
        .arg(scene.y())
        .arg(item->isVisible());
}

namespace {
void collect_named(QQuickItem* item, const QString& name, QList<QQuickItem*>& out) {
    if (!item) {
        return;
    }
    if (item->objectName() == name && item->isVisible()) {
        out.append(item);
    }
    for (auto* child : item->childItems()) {
        collect_named(child, name, out);
    }
}
} // namespace

bool lognotes_click_named(const QString& name, int index) {
    auto* w = window();
    if (!w) {
        return false;
    }
    QList<QQuickItem*> matches;
    collect_named(w->contentItem(), name, matches);
    if (index < 0 || index >= matches.size()) {
        return false;
    }
    auto* item = matches.at(index);
    const QPointF centre = item->mapToScene(QPointF(item->width() / 2.0, item->height() / 2.0));
    const QPointF global = w->mapToGlobal(centre.toPoint());
    QMouseEvent press(QEvent::MouseButtonPress, centre, global, Qt::LeftButton, Qt::LeftButton,
                      Qt::NoModifier);
    QCoreApplication::sendEvent(w, &press);
    QMouseEvent release(QEvent::MouseButtonRelease, centre, global, Qt::LeftButton, Qt::NoButton,
                        Qt::NoModifier);
    QCoreApplication::sendEvent(w, &release);
    return true;
}

QString lognotes_named_text(const QString& name, int index) {
    auto* w = window();
    if (!w) {
        return QString();
    }
    QList<QQuickItem*> matches;
    collect_named(w->contentItem(), name, matches);
    if (index < 0 || index >= matches.size()) {
        return QString();
    }
    auto* item = matches.at(index);
    // A Label carries `text`, an Image carries `source`; read whichever this item has.
    const QVariant text = item->property("text");
    if (text.isValid() && !text.toString().isEmpty()) {
        return text.toString();
    }
    return item->property("source").toString();
}

QString lognotes_editor_format(int index, int offset) {
    auto* w = window();
    if (!w) {
        return QString();
    }
    QList<QQuickItem*> editors;
    collect_editors(w->contentItem(), editors);
    if (index < 0 || index >= editors.size()) {
        return QString();
    }

    // cxx-qt gives QML a QQuickTextDocument; the formats live on the QTextDocument inside it.
    auto* wrapper = qobject_cast<QQuickTextDocument*>(
        editors.at(index)->property("textDocument").value<QObject*>());
    if (!wrapper || !wrapper->textDocument()) {
        return QString();
    }
    QTextDocument* document = wrapper->textDocument();
    if (offset < 0 || offset >= document->characterCount() - 1) {
        return QString();
    }

    // A highlighter sets formats on the block's *layout*, not on the document's fragments, so this is
    // the only place they can be read back from.
    const QTextBlock block = document->findBlock(offset);
    if (!block.isValid() || !block.layout()) {
        return QString();
    }
    const int inBlock = offset - block.position();
    QTextCharFormat format;
    for (const QTextLayout::FormatRange& range : block.layout()->formats()) {
        if (range.start <= inBlock && inBlock < range.start + range.length) {
            format = range.format;
            break;
        }
    }

    return QString("weight=%1 italic=%2 strike=%3 mono=%4 underline=%5")
        .arg(format.fontWeight())
        .arg(format.fontItalic() ? 1 : 0)
        .arg(format.fontStrikeOut() ? 1 : 0)
        .arg(format.hasProperty(QTextFormat::FontFamilies) ? 1 : 0)
        .arg(format.fontUnderline() ? 1 : 0);
}

bool lognotes_scroll_to_end() {
    auto* w = window();
    if (!w) {
        return false;
    }
    QList<QQuickItem*> columns;
    collect_named(w->contentItem(), QStringLiteral("column"), columns);
    if (columns.isEmpty()) {
        return false;
    }
    return QMetaObject::invokeMethod(columns.first(), "positionViewAtEnd");
}

bool lognotes_close() {
    auto* w = window();
    return w ? w->close() : false;
}

QString lognotes_journal_error() {
    auto* model = journal_model();
    return model ? model->property("error").toString() : QStringLiteral("<no model>");
}

bool lognotes_journal_ready() {
    auto* model = journal_model();
    return model && model->property("ready").toBool();
}

int lognotes_row_count() {
    auto* model = journal_model();
    return model ? model->rowCount() : -1;
}
