#include <lognotes-qt/driver_shim.h>

#include <QAbstractItemModel>
#include <QCoreApplication>
#include <QEventLoop>
#include <QImage>
#include <QKeyEvent>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QTimer>

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

int lognotes_row_count() {
    auto* model = journal_model();
    return model ? model->rowCount() : -1;
}
