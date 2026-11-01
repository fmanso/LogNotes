// Development-only helpers that let a test press real keys and look at the window.
// Compiled only when the `driver` feature is on, so the shipped app carries no test hooks.
//
// Free functions at global scope: cxx looks the symbols up unqualified, so a namespace here would need
// repeating in the bridge.

#pragma once

#include <QString>

class QQmlApplicationEngine;

// Remembers the engine's root object; every other call acts on it.
bool lognotes_attach(const QQmlApplicationEngine& engine);

// Types `text` through the window's focus item, as a keyboard would.
bool lognotes_type_text(const QString& text);

// Presses one key with Qt::KeyboardModifiers.
bool lognotes_press_key(int key, int modifiers);

// Writes the rendered window to `path` as PNG.
bool lognotes_save_png(const QString& path);

// Runs the event loop for `ms` so debounced saves and worker replies can land.
void lognotes_wait_ms(int ms);

// Object name (or class name) of whatever holds keyboard focus — where a keystroke would go.
QString lognotes_focus_name();

// How many rows the journal model currently has.
int lognotes_row_count();

// Clicks the centre of the window's index-th visible block editor (0-based).
bool lognotes_click_block(int index);

// Text currently shown in the index-th visible block editor, or empty if there is none.
QString lognotes_editor_text(int index);

QString lognotes_editor_debug(int index);

// Clicks the centre of the index-th visible item with this objectName.
bool lognotes_click_named(const QString& name, int index);

// Text shown by the index-th visible item with this objectName — a Label's text, an Image's source —
// or an empty string. How a rendered summary row and a drawn diagram are checked.
QString lognotes_named_text(const QString& name, int index);

// Clicks a link inside the index-th block row at rest, found with the view's own hit test. False when
// that row is being edited or holds no link.
bool lognotes_click_link(int index);

// The HTML the index-th block row renders at rest — what the markdown, or a coloured fence, produced.
QString lognotes_row_html(int index);

// How many rows the quick switcher is showing, or -1 when it is not open.
int lognotes_search_count();

// Puts the column at its end, which is what pulls in an older chunk. Qt Quick ignores synthetic wheel
// events under this compositor, so the harness drives the same end-of-column path directly.
bool lognotes_scroll_to_end();

// Closes the window, running its close handlers.
bool lognotes_close();

// The journal model's `error` text, empty when it opened fine.
QString lognotes_journal_error();

// Whether the journal model reports itself ready.
bool lognotes_journal_ready();
