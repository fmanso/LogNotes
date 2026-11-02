#include <lognotes-qt/driver_shim.h>

#include <QAbstractItemModel>
#include <QMetaObject>
#include <QDebug>
#include <QCoreApplication>
#include <QList>
#include <QEventLoop>
#include <QGuiApplication>
#include <QImage>
#include <QInputMethodEvent>
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
    // An input method only delivers to an active window, so bring it up before anything composes.
    if (auto* w = window()) {
        w->requestActivate();
    }
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

void lognotes_settle() {
    // Every save answers with a snapshot that resets the model, and each reset destroys and rebuilds the
    // delegates. Reading state while that is in flight is how a harness lies to itself, so wait until the
    // model has stopped changing for two ticks.
    auto* model = journal_model();
    if (!model) {
        return;
    }
    // An empty column is not a settled one: wait for the store to answer before judging anything.
    for (int spin = 0; spin < 100; ++spin) {
        if (model->property("ready").toBool() || !model->property("error").toString().isEmpty()) {
            break;
        }
        QEventLoop loop;
        QTimer::singleShot(80, &loop, &QEventLoop::quit);
        loop.exec();
    }

    // Just longer than the 250 ms debounce, so the save a keystroke scheduled has had its chance to run
    // before "nothing has changed" is taken to mean "settled".
    {
        QEventLoop loop;
        QTimer::singleShot(280, &loop, &QEventLoop::quit);
        loop.exec();
    }

    QVariant last = model->property("revision");
    for (int tick = 0; tick < 80; ++tick) {
        QEventLoop loop;
        QTimer::singleShot(60, &loop, &QEventLoop::quit);
        loop.exec();
        const QVariant now = model->property("revision");
        if (tick > 0 && now == last) {
            return;
        }
        last = now;
    }
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
    if (item->objectName() == QLatin1String("blockEditor")) {
        out.append(item);
    }
    // Walk the visual tree: ListView delegates live under the content item, not as QObject
    // children of the window.
    for (auto* child : item->childItems()) {
        collect_editors(child, out);
    }
}

// Names, whether or not the item is on screen — a row's rendered view exists even while its editor is the
// one being shown.
void collect_any(QQuickItem* item, const QString& name, QList<QQuickItem*>& out) {
    if (!item) {
        return;
    }
    if (item->objectName() == name) {
        out.append(item);
    }
    for (auto* child : item->childItems()) {
        collect_any(child, name, out);
    }
}

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

// The column's block rows in document order. A row holds both views of a block — the editor and the
// rendered text — so helpers address a row and then pick the view they need.
QList<QQuickItem*> block_rows() {
    QList<QQuickItem*> rows;
    if (auto* w = window()) {
        collect_named(w->contentItem(), QStringLiteral("blockRow"), rows);
    }
    QList<QQuickItem*> blocks;
    for (auto* row : rows) {
        if (row->property("isBlockRow").toBool()) {
            blocks.append(row);
        }
    }
    return blocks;
}

// Whichever view of the block is on screen — the editor or the rendered text. A row can be much taller
// than its text once a diagram is drawn under it, so a click goes here, not to the row's centre.
QQuickItem* text_surface_of(QQuickItem* row) {
    for (const char* name : {"blockEditor", "blockRendered"}) {
        QList<QQuickItem*> matches;
        collect_named(row, QString::fromLatin1(name), matches);
        if (!matches.isEmpty()) {
            return matches.first();
        }
    }
    return nullptr;
}

// A row's editor, whether or not it is the view currently on screen: its document still holds the raw
// text and its highlighter still styles it.
QQuickItem* editor_of(QQuickItem* row) {
    QList<QQuickItem*> editors;
    collect_editors(row, editors);
    return editors.isEmpty() ? nullptr : editors.first();
}
} // namespace

bool lognotes_click_block(int index) {
    return lognotes_click_block_at(index, 0.5);
}

bool lognotes_click_block_at(int index, double fraction) {
    auto* w = window();
    const auto rows = block_rows();
    if (!w || index < 0 || index >= rows.size()) {
        return false;
    }
    QQuickItem* item = text_surface_of(rows.at(index));
    if (!item) {
        item = rows.at(index);
    }
    const QPointF centre =
        item->mapToScene(QPointF(item->width() * fraction, item->height() / 2.0));
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
    const auto rows = block_rows();
    if (index < 0 || index >= rows.size()) {
        return {};
    }
    auto* editor = editor_of(rows.at(index));
    return editor ? editor->property("text").toString() : QString();
}

QString lognotes_editor_debug(int index) {
    const auto rows = block_rows();
    if (index < 0 || index >= rows.size()) {
        return QStringLiteral("<none>");
    }
    auto* item = editor_of(rows.at(index));
    if (!item) {
        return QStringLiteral("<no editor>");
    }
    const QPointF scene = item->mapToScene(QPointF(0, 0));
    return QStringLiteral("%1 w=%2 h=%3 at(%4,%5) visible=%6")
        .arg(item->property("text").toString())
        .arg(item->width())
        .arg(item->height())
        .arg(scene.x())
        .arg(scene.y())
        .arg(item->isVisible());
}

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

bool lognotes_click_link(int index) {
    auto* w = window();
    if (!w) {
        return false;
    }
    const auto rows = block_rows();
    if (index < 0 || index >= rows.size()) {
        return false;
    }

    // Only a row at rest draws anchors; the editor needs Ctrl+click.
    QList<QQuickItem*> rendered;
    collect_named(rows.at(index), QStringLiteral("blockRendered"), rendered);
    if (rendered.isEmpty()) {
        return false;
    }
    auto* item = rendered.first();

    // Walk the rendered text until the item's own hit test says there is a link under the point.
    for (qreal y = 2; y < item->height(); y += 4) {
        for (qreal x = 2; x < item->width(); x += 8) {
            QString link;
            if (!QMetaObject::invokeMethod(item, "linkAt", Q_RETURN_ARG(QString, link),
                                           Q_ARG(double, x), Q_ARG(double, y))
                || link.isEmpty()) {
                continue;
            }
            const QPointF scene = item->mapToScene(QPointF(x, y));
            const QPointF global = w->mapToGlobal(scene.toPoint());
            QMouseEvent press(QEvent::MouseButtonPress, scene, global, Qt::LeftButton,
                              Qt::LeftButton, Qt::NoModifier);
            QCoreApplication::sendEvent(w, &press);
            QMouseEvent release(QEvent::MouseButtonRelease, scene, global, Qt::LeftButton,
                                Qt::NoButton, Qt::NoModifier);
            QCoreApplication::sendEvent(w, &release);
            return true;
        }
    }
    return false;
}

bool lognotes_visible(const QString& name) {
    auto* w = window();
    if (!w) {
        return false;
    }
    QList<QQuickItem*> matches;
    collect_named(w->contentItem(), name, matches);
    return !matches.isEmpty();
}

QString lognotes_rendered_html() {
    auto* w = window();
    if (!w) {
        return QString();
    }
    QList<QQuickItem*> rendered;
    collect_named(w->contentItem(), QStringLiteral("blockRendered"), rendered);
    QString all;
    for (auto* item : rendered) {
        all += item->property("text").toString();
    }
    return all;
}

QString lognotes_row_html(int row) {
    const auto rows = block_rows();
    if (row < 0 || row >= rows.size()) {
        return QString();
    }
    QList<QQuickItem*> rendered;
    collect_any(rows.at(row), QStringLiteral("blockRendered"), rendered);
    return rendered.isEmpty() ? QString() : rendered.first()->property("text").toString();
}

int lognotes_search_count() {
    auto* w = window();
    if (!w) {
        return -1;
    }
    QList<QQuickItem*> matches;
    collect_named(w->contentItem(), QStringLiteral("searchList"), matches);
    if (matches.isEmpty()) {
        return -1;
    }
    return matches.first()->property("count").toInt();
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

QString lognotes_journal_property(const QString& name) {
    auto* model = journal_model();
    return model ? model->property(name.toUtf8().constData()).toString() : QString();
}

QString lognotes_journal_error() {
    auto* model = journal_model();
    return model ? model->property("error").toString() : QStringLiteral("<no model>");
}

bool lognotes_window_visible() {
    auto* w = window();
    return w && w->isVisible();
}

bool lognotes_journal_ready() {
    auto* model = journal_model();
    return model && model->property("ready").toBool();
}

bool lognotes_call_journal(const QString& method, const QString& argument) {
    auto* model = journal_model();
    if (!model) {
        return false;
    }
    return QMetaObject::invokeMethod(model,
                                     method.toUtf8().constData(),
                                     Q_ARG(QString, argument));
}

int lognotes_row_count() {
    auto* model = journal_model();
    return model ? model->rowCount() : -1;
}
